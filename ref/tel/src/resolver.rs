//! Schema resolution, per §8.2 of the TEL Specification.
//!
//! The resolver follows the five-step protocol: built-in lookup → cache
//! lookup → library lookup → URL fetch → failure. The network step is
//! pluggable via the `SchemaFetcher` trait so the crate carries no
//! mandatory HTTP dependency; an application that needs network resolution
//! supplies a fetcher backed by its own HTTP client.
//!
//! The library is indexed **per component** (BinTEL §8.1): the base
//! schema (the schema document with all `layer` compounds stripped) is
//! stored by its value hash in `base_library`, and each layer is stored
//! by its own value hash in `layer_library`. A multi-component
//! palimpsest signature is decoded against the union of these two
//! libraries; the first component is the base hash, the rest are layer
//! hashes, in order. The resolver then composes the recovered Layers
//! onto the base via `compose_schema` (§20.3) and returns the resulting
//! `Schema`.

use crate::{
    Schema, Layer, Struct, Document, Block, LineEndings, Compound,
    parse, construct_schema, builtin_tel_schema, type_assign, compose_schema,
};
use crate::bintel;
use crate::base256;
use std::collections::HashMap;

/// A signature identifies a composed schema as carried by the pragma:
/// either a URL (with or without a BASE-256 fragment) or a bare BASE-256
/// signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIdentifier {
    /// The URL, if the identifier carries one. May or may not include the
    /// fragment (`#…`) — the fragment is the BASE-256-encoded signature.
    pub url: Option<String>,
    /// The decoded signature bytes, if the identifier carries one.
    pub signature: Option<Vec<u8>>,
}

impl SchemaIdentifier {
    /// Parse a pragma identifier string. Recognises:
    ///
    /// - `http(s)://…` — URL without signature.
    /// - `http(s)://…#<BASE-256>` — URL with BASE-256 fragment signature.
    /// - `<BASE-256>` — bare signature.
    ///
    /// Returns `None` for inputs that match none of these forms (E122 at
    /// parse time).
    pub fn parse(s: &str) -> Option<Self> {
        if s.contains("://") {
            if let Some(idx) = s.find('#') {
                let url = &s[..idx];
                let frag = &s[idx + 1..];
                let sig = base256::decode(frag);
                Some(Self { url: Some(url.to_string()), signature: Some(sig) })
            } else {
                Some(Self { url: Some(s.to_string()), signature: None })
            }
        } else if !s.is_empty() && s.chars().all(|c| {
            c.is_ascii_digit() || c.is_ascii_alphabetic() || (c as u32) >= 0xA0
        }) {
            Some(Self { url: None, signature: Some(base256::decode(s)) })
        } else {
            None
        }
    }

    /// True when this identifier carries a signature (either as a fragment
    /// or as a bare signature).
    pub fn has_signature(&self) -> bool { self.signature.is_some() }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionError {
    /// Built-in lookup failed and no other source had the schema.
    NotFound,
    /// The fetched body did not parse as a TEL document.
    MalformedSchemaBody { detail: String },
    /// Signature verification failed: the fetched body's value hash does
    /// not match the signature carried by the identifier.
    SignatureMismatch { expected: Vec<u8>, actual: Vec<u8> },
    /// The fetcher reported a network or transport error.
    FetchError { detail: String },
    /// The identifier could not be parsed (would be E122 at TEL parse
    /// time, surfaced here for completeness).
    BadIdentifier,
}

/// Fetcher trait. Implementations may use any HTTP client (reqwest,
/// ureq, surf, etc.); a `Box<dyn SchemaFetcher>` is sufficient for most
/// uses.
pub trait SchemaFetcher {
    fn fetch(&self, url: &str) -> Result<String, String>;
}

/// A `SchemaFetcher` backed by an in-memory map. Useful in tests and for
/// applications that pre-load known schemas.
pub struct InMemoryFetcher {
    pub by_url: HashMap<String, String>,
}

impl InMemoryFetcher {
    pub fn new() -> Self { Self { by_url: HashMap::new() } }
    pub fn add(&mut self, url: &str, body: &str) {
        self.by_url.insert(url.to_string(), body.to_string());
    }
}

impl SchemaFetcher for InMemoryFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        self.by_url.get(url).cloned().ok_or_else(|| format!("no schema at {}", url))
    }
}

/// Schema resolver. Carries an optional fetcher, an in-memory cache
/// (signature → composed Schema), and a per-component library indexed
/// by each component's BinTEL value hash (§8.1).
pub struct Resolver<F: SchemaFetcher> {
    cache: HashMap<Vec<u8>, Schema>,
    base_library: HashMap<[u8; 32], Schema>,
    layer_library: HashMap<[u8; 32], Layer>,
    fetcher: Option<F>,
}

impl<F: SchemaFetcher> Resolver<F> {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            base_library: HashMap::new(),
            layer_library: HashMap::new(),
            fetcher: None,
        }
    }

    pub fn with_fetcher(fetcher: F) -> Self {
        Self {
            cache: HashMap::new(),
            base_library: HashMap::new(),
            layer_library: HashMap::new(),
            fetcher: Some(fetcher),
        }
    }

    /// Add a schema to the library, decomposing it into its base schema
    /// and any layers; each component is stored keyed by its own BinTEL
    /// value hash (§8.1). Returns the composed schema's full signature
    /// (palimpsest of base + layer hashes, per BinTEL §8.2). For a
    /// schema with no layers this is 32 bytes; with `n` layers it is
    /// `30 + 2(n+1)` bytes.
    pub fn add_to_library(&mut self, source: &str) -> Result<Vec<u8>, ResolutionError> {
        let parsed = parse(source);
        if !parsed.errors.is_empty() {
            return Err(ResolutionError::MalformedSchemaBody {
                detail: format!("{} parse errors", parsed.errors.len()),
            });
        }
        let ta = type_assign(&parsed.document, &builtin_tel_schema(), None);
        if !ta.errors.is_empty() {
            return Err(ResolutionError::MalformedSchemaBody {
                detail: format!("{} type-assignment errors", ta.errors.len()),
            });
        }
        let schema = construct_schema(&parsed.document);

        // Base component: schema document with `layer` compounds stripped.
        let base_hash = compute_base_hash(&parsed.document);
        let mut base_schema = schema.clone();
        base_schema.layers = Vec::new();
        self.base_library.insert(base_hash, base_schema);

        let mut component_hashes: Vec<[u8; 32]> = vec![base_hash];

        // Layer components: each `layer` compound in source order.
        // `schema.layers` already preserves source order (construct_schema
        // walks blocks/compounds top-down), so we walk both in lockstep.
        let mut layer_iter = schema.layers.iter();
        for block in &parsed.document.children {
            for c in &block.compounds {
                if c.keyword == "layer" {
                    let layer = layer_iter.next()
                        .expect("layer count matches layer-compound count");
                    let layer_hash = compute_layer_hash(c);
                    self.layer_library.insert(layer_hash, layer.clone());
                    component_hashes.push(layer_hash);
                }
            }
        }

        Ok(bintel::schema_signature_from_hashes(&component_hashes))
    }

    /// Add a base schema directly by its value hash. The caller is
    /// responsible for ensuring the hash matches the schema's content
    /// (the per-component encoding rule, BinTEL §8.1).
    pub fn add_base_to_library_with_hash(&mut self, hash: [u8; 32], base_schema: Schema) {
        self.base_library.insert(hash, base_schema);
    }

    /// Add a layer directly by its value hash. The caller is responsible
    /// for ensuring the hash matches the layer's content.
    pub fn add_layer_to_library_with_hash(&mut self, hash: [u8; 32], layer: Layer) {
        self.layer_library.insert(hash, layer);
    }

    /// All component hashes currently in the library (base and layer
    /// hashes combined), for diagnostics.
    pub fn library_hashes(&self) -> Vec<[u8; 32]> {
        let mut all: Vec<[u8; 32]> = self.base_library.keys().copied().collect();
        all.extend(self.layer_library.keys().copied());
        all
    }

    /// Resolve an identifier to a `Schema`, applying §8.2's five-step
    /// protocol.
    pub fn resolve(&mut self, identifier: &SchemaIdentifier) -> Result<Schema, ResolutionError> {
        // Step 1: built-in lookup.
        if let Some(sig) = &identifier.signature {
            let builtin = builtin_tel_schema();
            let builtin_hash = hex_decode_const(
                "df50abce267dc79106d4320f0879fb054236e8dce9efa04872fb5e2a6560fc52");
            if sig == &builtin_hash {
                return Ok(builtin);
            }
        }

        // Step 2: cache lookup.
        if let Some(sig) = &identifier.signature {
            if let Some(s) = self.cache.get(sig) {
                return Ok(s.clone());
            }
        }

        // Step 3: library lookup.
        if let Some(sig) = &identifier.signature {
            if sig.len() == 32 {
                // Single-component signature: the signature is the base
                // schema's value hash.
                let mut arr = [0u8; 32];
                arr.copy_from_slice(sig);
                if let Some(s) = self.base_library.get(&arr) {
                    return Ok(s.clone());
                }
            } else if sig.len() >= 34 && (sig.len() - 30) % 2 == 0 {
                // Multi-component palimpsest signature at cadence k=2.
                // Decompose against the combined bibliography.
                let bib = self.build_bibliography();
                let palimp = palimpsest::Palimpsest::from_bytes(sig.to_vec(), 2);
                if let Some(components) = palimpsest::decode(&palimp, &bib) {
                    if let Some(composed) = self.compose_from_components(&components) {
                        // Cache the composed result keyed by the full
                        // signature so subsequent lookups skip decode.
                        self.cache.insert(sig.clone(), composed.clone());
                        return Ok(composed);
                    }
                }
            }
        }

        // Step 4: URL fetch.
        if let Some(url) = &identifier.url {
            let fetcher = self.fetcher.as_ref().ok_or(ResolutionError::NotFound)?;
            let body = fetcher.fetch(url)
                .map_err(|detail| ResolutionError::FetchError { detail })?;
            let schema = parse_schema_body(&body)?;
            if let Some(expected_sig) = &identifier.signature {
                // Compute the actual signature by composing per-component
                // hashes from the fetched body. For a no-layer schema this
                // is a 32-byte single-component signature; for layered
                // schemas it is a 30 + 2n-byte palimpsest (BinTEL §8).
                let parsed = parse(&body);
                let actual_sig = compute_full_signature(&parsed.document);
                if expected_sig.as_slice() == actual_sig.as_slice() {
                    self.cache.insert(expected_sig.clone(), schema.clone());
                    return Ok(schema);
                }
                return Err(ResolutionError::SignatureMismatch {
                    expected: expected_sig.clone(),
                    actual: actual_sig,
                });
            }
            return Ok(schema);
        }

        // Step 5: failure.
        Err(ResolutionError::NotFound)
    }

    /// Given a decoded sequence of component hashes (first = base, rest
    /// = layers), look each up and compose into a single Schema. Returns
    /// None if any component is missing from the library or composition
    /// surfaces errors.
    fn compose_from_components(&self, components: &[palimpsest::Hash]) -> Option<Schema> {
        if components.is_empty() { return None; }
        let base = self.base_library.get(components[0].bytes())?.clone();
        let mut layers: Vec<Layer> = Vec::new();
        for h in &components[1..] {
            let layer = self.layer_library.get(h.bytes())?;
            layers.push(layer.clone());
        }
        let mut staged = base;
        staged.layers = layers;
        let (composed, errors) = compose_schema(&staged);
        if !errors.is_empty() { return None; }
        Some(composed)
    }

    fn build_bibliography(&self) -> palimpsest::Bibliography {
        let mut bib = palimpsest::Bibliography::new(2);
        for h in self.base_library.keys() {
            bib.add(palimpsest::Hash::new(*h));
        }
        for h in self.layer_library.keys() {
            bib.add(palimpsest::Hash::new(*h));
        }
        bib
    }
}

/// Compute the full composed signature (BinTEL §8.2 palimpsest) of a parsed
/// schema document: the base hash followed by each layer hash, in source
/// order. Returns 32 bytes for a no-layer schema, 30 + 2*n bytes for an
/// n-component schema (base + n-1 layers).
fn compute_full_signature(doc: &Document) -> Vec<u8> {
    let mut component_hashes: Vec<[u8; 32]> = vec![compute_base_hash(doc)];
    for block in &doc.children {
        for c in &block.compounds {
            if c.keyword == "layer" {
                component_hashes.push(compute_layer_hash(c));
            }
        }
    }
    bintel::schema_signature_from_hashes(&component_hashes)
}

/// Compute the BinTEL value hash of a schema document's **base** (the
/// document with all `layer` compounds removed), per BinTEL §8.1.
fn compute_base_hash(doc: &Document) -> [u8; 32] {
    let base_doc = Document {
        interpreter_directive: doc.interpreter_directive.clone(),
        pragma: doc.pragma.clone(),
        line_endings: doc.line_endings,
        children: doc.children.iter().map(|b| Block {
            comments: b.comments.clone(),
            tabulation: b.tabulation.clone(),
            compounds: b.compounds.iter()
                .filter(|c| c.keyword != "layer")
                .cloned().collect(),
            trailing_blank_lines: b.trailing_blank_lines,
        }).collect(),
    };
    bintel::value_hash(&base_doc, &builtin_tel_schema())
}

/// Compute the BinTEL value hash of a single `layer` compound, encoded
/// as a virtual document whose root Struct is the tel-schema
/// `layer-body` Definition, per BinTEL §8.1.
fn compute_layer_hash(layer_compound: &Compound) -> [u8; 32] {
    let layer_doc = Document {
        interpreter_directive: None,
        pragma: None,
        line_endings: LineEndings::LF,
        children: layer_compound.children.clone(),
    };
    let tel = builtin_tel_schema();
    let layer_body_def = tel.types.iter().find(|d| d.name == "layer-body")
        .expect("builtin tel-schema must define layer-body");
    let synth_schema = Schema {
        name: "tel-layer-body".to_string(),
        document: Struct {
            members: layer_body_def.members.clone(),
            validators: layer_body_def.validators.clone(),
        },
        layers: Vec::new(),
        sigil: tel.sigil,
        types: tel.types.clone(), scalars: Vec::new(),
    };
    bintel::value_hash(&layer_doc, &synth_schema)
}

fn parse_schema_body(body: &str) -> Result<Schema, ResolutionError> {
    let parsed = parse(body);
    if !parsed.errors.is_empty() {
        return Err(ResolutionError::MalformedSchemaBody {
            detail: format!("{} parse errors", parsed.errors.len()),
        });
    }
    let ta = type_assign(&parsed.document, &builtin_tel_schema(), None);
    if !ta.errors.is_empty() {
        return Err(ResolutionError::MalformedSchemaBody {
            detail: format!("{} type-assignment errors", ta.errors.len()),
        });
    }
    Ok(construct_schema(&parsed.document))
}

fn hex_decode_const(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i+2], 16).unwrap())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Member;

    #[test]
    fn identifier_parses_url_without_signature() {
        let id = SchemaIdentifier::parse("https://example.org/x").unwrap();
        assert_eq!(id.url.as_deref(), Some("https://example.org/x"));
        assert!(id.signature.is_none());
    }

    #[test]
    fn identifier_parses_url_with_signature_fragment() {
        let id = SchemaIdentifier::parse("https://example.org/x#abcd").unwrap();
        assert_eq!(id.url.as_deref(), Some("https://example.org/x"));
        assert!(id.signature.is_some());
    }

    #[test]
    fn identifier_parses_bare_signature() {
        let id = SchemaIdentifier::parse("ḀḁЂЃĄąĆćȈȉ").unwrap();
        assert!(id.url.is_none());
        assert!(id.signature.is_some());
    }

    #[test]
    fn identifier_rejects_garbage() {
        assert!(SchemaIdentifier::parse("").is_none());
        assert!(SchemaIdentifier::parse("not a url and not a signature !").is_none());
    }

    #[test]
    fn resolver_library_lookup_after_add_to_library() {
        // Schema with no layers: signature is exactly the base hash (32 bytes).
        let src = "tel 1.0\n\nname my-schema\n\ndocument\n  field x string\n";
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        let sig = r.add_to_library(src).expect("add_to_library should succeed");
        assert_eq!(sig.len(), 32, "no-layer signature is 32 bytes");
        let id = SchemaIdentifier { url: None, signature: Some(sig) };
        let s = r.resolve(&id).expect("library lookup should succeed");
        assert_eq!(s.name, "my-schema");
    }

    #[test]
    fn resolver_returns_builtin_for_tel_schema_hash() {
        let pinned_hex = "df50abce267dc79106d4320f0879fb054236e8dce9efa04872fb5e2a6560fc52";
        let sig = hex_decode_const(pinned_hex);
        let id = SchemaIdentifier { url: None, signature: Some(sig) };
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        let s = r.resolve(&id).unwrap();
        assert_eq!(s.name, "tel-schema");
    }

    #[test]
    fn resolver_fetches_url_when_signature_absent() {
        let body = "name greeting\n\ndocument\n  field x string\n";
        let mut fetcher = InMemoryFetcher::new();
        fetcher.add("https://example.org/x", body);
        let mut r = Resolver::with_fetcher(fetcher);
        let id = SchemaIdentifier::parse("https://example.org/x").unwrap();
        let s = r.resolve(&id).unwrap();
        assert_eq!(s.name, "greeting");
    }

    #[test]
    fn resolver_reports_not_found_with_no_signature_or_fetcher() {
        let id = SchemaIdentifier::parse("https://example.org/x").unwrap();
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        assert!(matches!(r.resolve(&id), Err(ResolutionError::NotFound)));
    }

    #[test]
    fn resolver_fetch_failure_propagates() {
        let mut r = Resolver::with_fetcher(InMemoryFetcher::new());
        let id = SchemaIdentifier::parse("https://example.org/x").unwrap();
        let err = r.resolve(&id).unwrap_err();
        assert!(matches!(err, ResolutionError::FetchError { .. }));
    }

    #[test]
    fn resolver_signature_mismatch_is_reported() {
        let body = "name greeting\n\ndocument\n  field x string\n";
        let mut fetcher = InMemoryFetcher::new();
        fetcher.add("https://example.org/x", body);
        let mut r = Resolver::with_fetcher(fetcher);
        let id = SchemaIdentifier {
            url: Some("https://example.org/x".to_string()),
            signature: Some(vec![0u8; 32]),
        };
        let err = r.resolve(&id).unwrap_err();
        assert!(matches!(err, ResolutionError::SignatureMismatch { .. }));
    }

    #[test]
    fn resolver_decomposes_layered_signature() {
        // Schema with one layer: signature is 30 + 2*2 = 34 bytes.
        // Round-trip: add to library, resolve by the composed signature,
        // verify the composed Schema exposes both base and layer fields.
        let layered_src = "\
tel 1.0

name layered-demo

document
  field x string

layer
  name extra
  overlay
    field y string
";
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        let sig = r.add_to_library(layered_src).expect("add_to_library succeeds");
        assert_eq!(sig.len(), 34, "one-layer signature is 34 bytes (30 + 2*2)");
        let id = SchemaIdentifier { url: None, signature: Some(sig) };
        let s = r.resolve(&id).expect("layered signature resolves");
        // compose_schema flattens layers into the document; no residual layers.
        assert!(s.layers.is_empty(), "composed schema has no residual layers");
        let names: Vec<&str> = s.document.members.iter().filter_map(|m| match m {
            Member::Field(f) => Some(f.keyword.as_str()),
            _ => None,
        }).collect();
        assert!(names.contains(&"x"), "base field x present: {:?}", names);
        assert!(names.contains(&"y"), "layer field y present: {:?}", names);
    }

    #[test]
    fn resolver_url_fetch_verifies_multi_component_signature() {
        // A layered schema is served by URL with its full palimpsest
        // signature carried in the fragment. The resolver fetches the
        // body, computes the full signature from base + each layer's
        // BinTEL hash, and accepts on a byte-for-byte match.
        let layered_src = "\
tel 1.0

name url-layered

document
  field x string

layer
  name extra
  overlay
    field y optional string
";
        // First compute the expected signature using add_to_library; this
        // doesn't add the schema to the library — we discard the resolver
        // afterwards. We're only using it to get the signature bytes.
        let mut sig_resolver: Resolver<InMemoryFetcher> = Resolver::new();
        let expected_sig = sig_resolver.add_to_library(layered_src).unwrap();
        assert_eq!(expected_sig.len(), 34, "one-layer signature is 34 bytes");

        let mut fetcher = InMemoryFetcher::new();
        fetcher.add("https://example.org/layered", layered_src);
        let mut r = Resolver::with_fetcher(fetcher);
        let id = SchemaIdentifier {
            url: Some("https://example.org/layered".to_string()),
            signature: Some(expected_sig),
        };
        let s = r.resolve(&id).expect("multi-component URL fetch should verify");
        assert_eq!(s.name, "url-layered");
    }

    #[test]
    fn resolver_layered_signature_misses_when_layer_absent() {
        // Add the schema, then construct a 34-byte signature whose layer
        // component isn't in the library. Resolution falls through to
        // NotFound (no URL fetcher configured).
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        let src = "\
tel 1.0

name with-layer

document
  field x string

layer
  name extra
  overlay
    field y string
";
        let sig = r.add_to_library(src).unwrap();
        // Now drop the layer from the library and re-attempt resolution.
        let layer_keys: Vec<[u8; 32]> = r.layer_library.keys().copied().collect();
        for k in &layer_keys { r.layer_library.remove(k); }
        let id = SchemaIdentifier { url: None, signature: Some(sig) };
        let err = r.resolve(&id).unwrap_err();
        assert!(matches!(err, ResolutionError::NotFound));
    }
}
