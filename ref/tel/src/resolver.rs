//! Schema resolution, per §8.2 of the TEL Specification.
//!
//! The resolver follows the five-step protocol: built-in lookup → cache
//! lookup → library lookup → LIRA resolution → failure. The network step
//! is pluggable via the `SchemaFetcher` trait so the crate carries no
//! mandatory network dependency; an application that needs network
//! resolution supplies a fetcher backed by its own LIRA client. Bare
//! (selector-less) references are local-only by design and never reach
//! the fetcher.
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
    Schema, Layer, Document,
    parse, construct_schema, builtin_tels, builtin_tels_value_hash,
    type_assign, compose_schema,
};
use crate::bintel;
use crate::base256;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Lazily-computed full schema signature for the built-in tels
/// (33 bytes: 32-byte BLAKE3-256 hash of tels.tel + cadence trailer).
fn builtin_tels_signature() -> Vec<u8> {
    static CACHE: OnceLock<Vec<u8>> = OnceLock::new();
    CACHE.get_or_init(|| {
        bintel::schema_signature_from_hashes(&[builtin_tels_value_hash()])
    }).clone()
}

/// The canonical, version-pinned LIRA coordinate of the built-in `tels`
/// meta-schema (§8.1). The pin advances only when the TEL specification
/// is revised.
pub const BUILTIN_TELS_REFERENCE: &str = "specification.tel/tels:1.0.0";

/// The schema identification carried by a pragma (§8.1): a LIRA
/// reference, layer selections, and/or a schema signature, which
/// together read as a single claim — *this document uses the referenced
/// schema, with these layers, as attested by this signature.*
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIdentifier {
    /// The LIRA reference (`domain/name`, optionally with a `:version` or
    /// `:tag` selector), verbatim, if the pragma carries one.
    pub reference: Option<String>,
    /// Selected layer names, in pragma order, without their `+` prefixes.
    pub layers: Vec<String>,
    /// The decoded signature bytes, if the pragma carries a signature.
    pub signature: Option<Vec<u8>>,
}

impl SchemaIdentifier {
    /// Build a `SchemaIdentifier` from a parsed pragma. Returns `None`
    /// when the pragma carries no schema identification at all.
    pub fn from_pragma(pragma: &crate::Pragma) -> Option<Self> {
        if pragma.reference.is_none() && pragma.signature.is_none() {
            return None;
        }
        Some(Self {
            reference: pragma.reference.clone(),
            layers: pragma.layers.clone(),
            signature: pragma.signature.as_deref().map(base256::decode),
        })
    }

    /// Parse a single identification phrase. Recognises:
    ///
    /// - `<domain>/<name>[:<version>|:<tag>]` — LIRA reference.
    /// - `<BASE-256>` — bare signature.
    ///
    /// Returns `None` for inputs that match neither form (E121 at parse
    /// time).
    pub fn parse(s: &str) -> Option<Self> {
        if s.contains('/') && s.chars().count() > 1 {
            if crate::is_valid_reference(s) {
                Some(Self { reference: Some(s.to_string()), layers: Vec::new(), signature: None })
            } else {
                None
            }
        } else if crate::is_valid_signature(s) {
            Some(Self { reference: None, layers: Vec::new(), signature: Some(base256::decode(s)) })
        } else {
            None
        }
    }

    /// True when this identification carries a signature.
    pub fn has_signature(&self) -> bool { self.signature.is_some() }

    /// True when this identification's reference carries a `:version` or
    /// `:tag` selector. A selector-form reference is globally resolvable;
    /// a bare reference is local-only by design (§8.2).
    pub fn has_selector(&self) -> bool {
        self.reference.as_deref().is_some_and(|r| r.contains(':'))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionError {
    /// Built-in lookup failed and no other source had the schema.
    NotFound,
    /// The resolved body did not parse as a TEL document.
    MalformedSchemaBody { detail: String },
    /// Signature verification failed: the resolved body's composed
    /// signature does not match the signature carried by the
    /// identification.
    SignatureMismatch { expected: Vec<u8>, actual: Vec<u8> },
    /// The fetcher reported a network or transport error.
    FetchError { detail: String },
    /// A selected layer name matches no layer declared by the resolved
    /// schema (§8.1).
    UnknownLayer { name: String },
    /// The identification could not be parsed (would be E121 at TEL
    /// parse time, surfaced here for completeness).
    BadIdentifier,
}

/// Fetcher trait: resolves a selector-form LIRA reference
/// (`domain/name:version` or `domain/name:tag`) to the released
/// `tels`-conforming TEL body. Implementations wrap a LIRA client (local
/// store first, then network resolution) and MUST verify the release's
/// manifest signature — a local store index alone is not authoritative
/// (§8.2). A `Box<dyn SchemaFetcher>` is sufficient for most uses.
pub trait SchemaFetcher {
    fn fetch(&self, reference: &str) -> Result<String, String>;
}

/// A `SchemaFetcher` backed by an in-memory map keyed by reference.
/// Useful in tests and for applications that pre-load known schemas.
pub struct InMemoryFetcher {
    pub by_reference: HashMap<String, String>,
}

impl InMemoryFetcher {
    pub fn new() -> Self { Self { by_reference: HashMap::new() } }
    pub fn add(&mut self, reference: &str, body: &str) {
        self.by_reference.insert(reference.to_string(), body.to_string());
    }
}

impl SchemaFetcher for InMemoryFetcher {
    fn fetch(&self, reference: &str) -> Result<String, String> {
        self.by_reference.get(reference).cloned()
            .ok_or_else(|| format!("no schema at {}", reference))
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
    /// (palimpsest of base + layer hashes at the BinTEL-pinned parameters
    /// `(H, k_i, k_r) = (32, 4, 2)`, per BinTEL §8.2). For a schema with no
    /// layers this is 33 bytes; with `n` layers it is `37 + 2·(n − 1)` bytes
    /// (37, 39, 41, … for n = 1, 2, 3 layers).
    pub fn add_to_library(&mut self, source: &str) -> Result<Vec<u8>, ResolutionError> {
        let parsed = parse(source);
        if !parsed.errors.is_empty() {
            return Err(ResolutionError::MalformedSchemaBody {
                detail: format!("{} parse errors", parsed.errors.len()),
            });
        }
        let ta = type_assign(&parsed.document, &builtin_tels(), None);
        if !ta.errors.is_empty() {
            return Err(ResolutionError::MalformedSchemaBody {
                detail: format!("{} type-assignment errors", ta.errors.len()),
            });
        }
        Ok(self.add_components_from_document(&parsed.document))
    }

    /// Add a schema to the library from its BinTEL encoding (a complete
    /// BinTEL document whose schema is tels and whose content is a
    /// TEL schema document). Decodes the bytes under the hardwired
    /// tels axiom, decomposes the result into base + layer
    /// components, and populates the library. Returns the resulting full
    /// composed signature, matching `add_to_library` for the equivalent
    /// TEL source.
    ///
    /// This is the BinTEL counterpart to `add_to_library`. The bytes
    /// produced by `bintel::schema_to_bintel(&schema_doc)` round-trip
    /// through this function and produce the same signature as
    /// `add_to_library(&schema_source)` for the same logical schema.
    pub fn add_bintel_to_library(&mut self, bintel_bytes: &[u8]) -> Result<Vec<u8>, ResolutionError> {
        let decoded = bintel::decode_document(bintel_bytes, &builtin_tels())
            .map_err(|e| ResolutionError::MalformedSchemaBody {
                detail: format!("BinTEL decode error {:?}: {}", e.code, e.context),
            })?;
        // The decoded bytes must declare themselves a tels document:
        // the carried signature must be tels's signature.
        if decoded.signature != builtin_tels_signature() {
            return Err(ResolutionError::SignatureMismatch {
                expected: builtin_tels_signature(),
                actual: decoded.signature,
            });
        }
        // Validate the decoded schema document under tels — catches any
        // structural issues that the BinTEL decoder did not detect (e.g.,
        // missing required scalars after composition).
        let ta = type_assign(&decoded.document, &builtin_tels(), None);
        if !ta.errors.is_empty() {
            return Err(ResolutionError::MalformedSchemaBody {
                detail: format!("{} type-assignment errors against tels", ta.errors.len()),
            });
        }
        Ok(self.add_components_from_document(&decoded.document))
    }

    /// Shared implementation: given a validated TEL schema document,
    /// decompose it into base + layer components (per BinTEL §8.1),
    /// populate the library, and return the composed signature.
    fn add_components_from_document(&mut self, doc: &Document) -> Vec<u8> {
        let schema = construct_schema(doc);
        // Base component: schema document with `layer` compounds stripped.
        let base_hash = bintel::schema_base_hash(doc);
        let mut base_schema = schema.clone();
        base_schema.layers = Vec::new();
        self.base_library.insert(base_hash, base_schema);

        let mut component_hashes: Vec<[u8; 32]> = vec![base_hash];

        // Layer components: each `layer` compound in source order.
        // `schema.layers` already preserves source order, so we walk
        // both in lockstep.
        let mut layer_iter = schema.layers.iter();
        for block in &doc.children {
            for c in &block.compounds {
                if c.keyword == "layer" {
                    let layer = layer_iter.next()
                        .expect("layer count matches layer-compound count");
                    let layer_hash = bintel::schema_layer_hash(c);
                    self.layer_library.insert(layer_hash, layer.clone());
                    component_hashes.push(layer_hash);
                }
            }
        }

        bintel::schema_signature_from_hashes(&component_hashes)
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

    /// Resolve a schema identification to a `Schema`, applying §8.2's
    /// five-step protocol.
    pub fn resolve(&mut self, identifier: &SchemaIdentifier) -> Result<Schema, ResolutionError> {
        // Step 1: built-in lookup. The tels built-in is identified by
        // its single-component signature (33 bytes: 32-byte BLAKE3-256 hash
        // of the canonical tels.tel + cadence trailer), or by its pinned
        // LIRA coordinate (§8.1). Neither form requires network access.
        if identifier.reference.as_deref() == Some(BUILTIN_TELS_REFERENCE) {
            return Ok(builtin_tels());
        }
        if let Some(sig) = &identifier.signature {
            if sig.len() == 33 && sig == &builtin_tels_signature() {
                return Ok(builtin_tels());
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
            if sig.len() == 33 {
                // Single-component signature: 32-byte base hash + cadence
                // trailer. Strip the trailer and look up the base hash.
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&sig[..32]);
                if let Some(s) = self.base_library.get(&arr) {
                    return Ok(s.clone());
                }
            } else if sig.len() >= 37 && (sig.len() - 37) % 2 == 0 {
                // Multi-component palimpsest signature at the BinTEL-pinned
                // parameters (k_i=4, k_r=2). Decompose against the combined
                // bibliography indexed at both 4-byte and 2-byte prefixes.
                let bib = self.build_bibliography();
                let palimp = palimpsest::Palimpsest::from_bytes(sig.to_vec());
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

        // Step 4: LIRA resolution. Only a selector-form reference
        // (`:version` or `:tag`) reaches the fetcher: a bare reference is
        // local-only by design and MUST NOT trigger network resolution
        // (§8.2), and a signature alone encodes no coordinate to fetch by.
        if identifier.has_selector() {
            if let (Some(reference), Some(fetcher)) =
                (&identifier.reference, self.fetcher.as_ref())
            {
                let body = fetcher.fetch(reference)
                    .map_err(|detail| ResolutionError::FetchError { detail })?;
                validate_schema_body(&body)?;
                let parsed = parse(&body);
                let (composed, actual_sig) =
                    compose_with_selected_layers(&parsed.document, &identifier.layers)?;
                if let Some(expected_sig) = &identifier.signature {
                    // The signature is authoritative and MUST include the
                    // selected layers: the composed signature over the base
                    // plus the selected layers' hashes, in declaration
                    // order, must match byte-for-byte (§8.1, §8.2).
                    if expected_sig.as_slice() != actual_sig.as_slice() {
                        return Err(ResolutionError::SignatureMismatch {
                            expected: expected_sig.clone(),
                            actual: actual_sig,
                        });
                    }
                }
                self.cache.insert(actual_sig, composed.clone());
                return Ok(composed);
            }
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
        let mut arr = [0u8; 32];
        if components[0].len() != 32 { return None; }
        arr.copy_from_slice(components[0].bytes());
        let base = self.base_library.get(&arr)?.clone();
        let mut layers: Vec<Layer> = Vec::new();
        for h in &components[1..] {
            if h.len() != 32 { return None; }
            let mut larr = [0u8; 32];
            larr.copy_from_slice(h.bytes());
            let layer = self.layer_library.get(&larr)?;
            layers.push(layer.clone());
        }
        let mut staged = base;
        staged.layers = layers;
        let (composed, errors) = compose_schema(&staged);
        if !errors.is_empty() { return None; }
        Some(composed)
    }

    fn build_bibliography(&self) -> palimpsest::Bibliography {
        let mut bib = palimpsest::Bibliography::for_cadences(
            bintel::SIGNATURE_INITIAL_CADENCE,
            bintel::SIGNATURE_REGULAR_CADENCE,
        );
        for h in self.base_library.keys() {
            bib.add(palimpsest::Hash::from(*h));
        }
        for h in self.layer_library.keys() {
            bib.add(palimpsest::Hash::from(*h));
        }
        bib
    }
}


fn validate_schema_body(body: &str) -> Result<(), ResolutionError> {
    let parsed = parse(body);
    if !parsed.errors.is_empty() {
        return Err(ResolutionError::MalformedSchemaBody {
            detail: format!("{} parse errors", parsed.errors.len()),
        });
    }
    let ta = type_assign(&parsed.document, &builtin_tels(), None);
    if !ta.errors.is_empty() {
        return Err(ResolutionError::MalformedSchemaBody {
            detail: format!("{} type-assignment errors", ta.errors.len()),
        });
    }
    Ok(())
}

/// True when the selected layer names appear in the schema's declaration
/// order (§8.1). A selection out of declaration order is **E124** at
/// parse time; its recovery is to reorder and continue, which
/// `compose_with_selected_layers` performs unconditionally.
pub fn layers_in_declaration_order(doc: &Document, selected: &[String]) -> bool {
    let declared = declared_layer_names(doc);
    let mut last = 0usize;
    for name in selected {
        match declared.iter().position(|d| d == name) {
            Some(pos) if pos + 1 > last => last = pos + 1,
            Some(_) => return false,
            None => return false,
        }
    }
    true
}

fn declared_layer_names(doc: &Document) -> Vec<String> {
    construct_schema(doc).layers.iter().map(|l| l.name.clone()).collect()
}

/// Compose a validated schema document with only the selected layers
/// (§8.1). Layer names are matched against the declared layer names;
/// composition follows the schema's declaration order regardless of
/// selection order (E124 recovery). An empty selection composes the base
/// schema alone: layers are OPTIONAL, and a document opts into exactly
/// the layers it names. Returns the composed `Schema` together with its
/// composed signature (the palimpsest of the base hash plus the selected
/// layers' hashes, in declaration order).
pub fn compose_with_selected_layers(
    doc: &Document,
    selected: &[String],
) -> Result<(Schema, Vec<u8>), ResolutionError> {
    let schema = construct_schema(doc);

    // Declared layers in declaration order, each with its value hash,
    // walking layer compounds and `schema.layers` in lockstep (both
    // preserve source order).
    let mut declared: Vec<(String, [u8; 32], Layer)> = Vec::new();
    let mut layer_iter = schema.layers.iter();
    for block in &doc.children {
        for c in &block.compounds {
            if c.keyword == "layer" {
                let layer = layer_iter.next()
                    .expect("layer count matches layer-compound count");
                declared.push((layer.name.clone(), bintel::schema_layer_hash(c), layer.clone()));
            }
        }
    }

    for name in selected {
        if !declared.iter().any(|(n, _, _)| n == name) {
            return Err(ResolutionError::UnknownLayer { name: name.clone() });
        }
    }

    let mut component_hashes: Vec<[u8; 32]> = vec![bintel::schema_base_hash(doc)];
    let mut layers: Vec<Layer> = Vec::new();
    for (name, hash, layer) in &declared {
        if selected.iter().any(|s| s == name) {
            component_hashes.push(*hash);
            layers.push(layer.clone());
        }
    }

    let mut staged = schema;
    staged.layers = layers;
    let (composed, errors) = compose_schema(&staged);
    if !errors.is_empty() {
        return Err(ResolutionError::MalformedSchemaBody {
            detail: format!("{} layer-composition errors", errors.len()),
        });
    }
    Ok((composed, bintel::schema_signature_from_hashes(&component_hashes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Member;

    #[test]
    fn identifier_parses_bare_reference() {
        let id = SchemaIdentifier::parse("example.org/x").unwrap();
        assert_eq!(id.reference.as_deref(), Some("example.org/x"));
        assert!(id.signature.is_none());
        assert!(!id.has_selector());
    }

    #[test]
    fn identifier_parses_selector_references() {
        let id = SchemaIdentifier::parse("example.org/x:1.2.0").unwrap();
        assert_eq!(id.reference.as_deref(), Some("example.org/x:1.2.0"));
        assert!(id.has_selector());
        let id = SchemaIdentifier::parse("specification.tel/jdk:jdk-19").unwrap();
        assert!(id.has_selector());
    }

    #[test]
    fn identifier_parses_bare_signature() {
        let sig33: String = "Ḁ".repeat(33);
        let id = SchemaIdentifier::parse(&sig33).unwrap();
        assert!(id.reference.is_none());
        assert!(id.signature.is_some());
    }

    #[test]
    fn identifier_rejects_garbage() {
        assert!(SchemaIdentifier::parse("").is_none());
        assert!(SchemaIdentifier::parse("not a reference and not a signature !").is_none());
        // URLs are no longer a schema-identification form.
        assert!(SchemaIdentifier::parse("https://example.org/x").is_none());
        // A short BASE-256 string is not a well-formed signature.
        assert!(SchemaIdentifier::parse("ḀḁЂЃĄąĆćȈȉ").is_none());
        // Selector must be a version or a tag.
        assert!(SchemaIdentifier::parse("example.org/x:01.2.3").is_none());
        assert!(SchemaIdentifier::parse("example.org/x:1.2").is_none());
    }

    #[test]
    fn resolver_library_lookup_after_add_to_library() {
        // Schema with no layers: signature is 33 bytes (32-byte base hash +
        // cadence trailer).
        let src = "tel 1.0\n\nname my-schema\n\ndocument\n  field x String\n";
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        let sig = r.add_to_library(src).expect("add_to_library should succeed");
        assert_eq!(sig.len(), 33, "no-layer signature is 33 bytes");
        let id = SchemaIdentifier { reference: None, layers: vec![], signature: Some(sig) };
        let s = r.resolve(&id).expect("library lookup should succeed");
        assert_eq!(s.name, "my-schema");
    }

    #[test]
    fn resolver_returns_builtin_for_tels_hash() {
        // Construct the expected signature dynamically (the BLAKE3-256 hash
        // of tels.tel is computed at runtime; once pinned, callers can
        // copy it into a const).
        let sig = super::builtin_tels_signature();
        assert_eq!(sig.len(), 33);
        let id = SchemaIdentifier { reference: None, layers: vec![], signature: Some(sig) };
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        let s = r.resolve(&id).unwrap();
        assert_eq!(s.name, "tels");
    }

    #[test]
    fn resolver_returns_builtin_for_pinned_coordinate() {
        // §8.2 step 1: the pinned coordinate resolves to the built-in
        // without network access (no fetcher configured).
        let id = SchemaIdentifier::parse(BUILTIN_TELS_REFERENCE).unwrap();
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        let s = r.resolve(&id).unwrap();
        assert_eq!(s.name, "tels");
    }

    #[test]
    fn resolver_fetches_selector_reference_when_signature_absent() {
        let body = "name greeting\n\ndocument\n  field x String\n";
        let mut fetcher = InMemoryFetcher::new();
        fetcher.add("example.org/greeting:1.0.0", body);
        let mut r = Resolver::with_fetcher(fetcher);
        let id = SchemaIdentifier::parse("example.org/greeting:1.0.0").unwrap();
        let s = r.resolve(&id).unwrap();
        assert_eq!(s.name, "greeting");
    }

    #[test]
    fn resolver_never_fetches_bare_reference() {
        // A bare (selector-less) reference is local-only by design (§8.2):
        // even with a fetcher that could serve it, resolution must not
        // reach the network.
        let body = "name greeting\n\ndocument\n  field x String\n";
        let mut fetcher = InMemoryFetcher::new();
        fetcher.add("example.org/greeting", body);
        let mut r = Resolver::with_fetcher(fetcher);
        let id = SchemaIdentifier::parse("example.org/greeting").unwrap();
        assert!(matches!(r.resolve(&id), Err(ResolutionError::NotFound)));
    }

    #[test]
    fn resolver_reports_not_found_with_no_signature_or_fetcher() {
        let id = SchemaIdentifier::parse("example.org/x:1.0.0").unwrap();
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        assert!(matches!(r.resolve(&id), Err(ResolutionError::NotFound)));
    }

    #[test]
    fn resolver_fetch_failure_propagates() {
        let mut r = Resolver::with_fetcher(InMemoryFetcher::new());
        let id = SchemaIdentifier::parse("example.org/x:1.0.0").unwrap();
        let err = r.resolve(&id).unwrap_err();
        assert!(matches!(err, ResolutionError::FetchError { .. }));
    }

    #[test]
    fn resolver_signature_mismatch_is_reported() {
        let body = "name greeting\n\ndocument\n  field x String\n";
        let mut fetcher = InMemoryFetcher::new();
        fetcher.add("example.org/greeting:1.0.0", body);
        let mut r = Resolver::with_fetcher(fetcher);
        // 33-byte all-zero signature: well-formed length but unlikely to
        // match any real BLAKE3 hash.
        let id = SchemaIdentifier {
            reference: Some("example.org/greeting:1.0.0".to_string()),
            layers: vec![],
            signature: Some(vec![0u8; 33]),
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
  field x String

layer
  name extra
  overlay
    field y String
";
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        let sig = r.add_to_library(layered_src).expect("add_to_library succeeds");
        assert_eq!(sig.len(), 37, "two-component signature is 37 bytes (32 + 4 + 1)");
        let id = SchemaIdentifier { reference: None, layers: vec![], signature: Some(sig) };
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
    fn resolver_lira_fetch_verifies_signature_with_selected_layers() {
        // A layered schema is served by a selector-form reference with the
        // full palimpsest signature carried in the pragma. The resolver
        // fetches the body, composes the selected layers, computes the
        // composed signature from base + each selected layer's BinTEL
        // hash, and accepts on a byte-for-byte match (§8.1: the signature
        // MUST include the selected layers).
        let layered_src = "\
tel 1.0

name url-layered

document
  field x String

layer
  name extra
  overlay
    field y String optional
";
        // First compute the expected signature using add_to_library; this
        // doesn't add the schema to the library — we discard the resolver
        // afterwards. We're only using it to get the signature bytes.
        let mut sig_resolver: Resolver<InMemoryFetcher> = Resolver::new();
        let expected_sig = sig_resolver.add_to_library(layered_src).unwrap();
        assert_eq!(expected_sig.len(), 37, "two-component signature is 37 bytes");

        let mut fetcher = InMemoryFetcher::new();
        fetcher.add("example.org/url-layered:1.0.0", layered_src);
        let mut r = Resolver::with_fetcher(fetcher);
        let id = SchemaIdentifier {
            reference: Some("example.org/url-layered:1.0.0".to_string()),
            layers: vec!["extra".to_string()],
            signature: Some(expected_sig),
        };
        let s = r.resolve(&id).expect("LIRA fetch with selected layers should verify");
        assert_eq!(s.name, "url-layered");
        // compose_schema flattens layers into the document.
        let names: Vec<&str> = s.document.members.iter().filter_map(|m| match m {
            Member::Field(f) => Some(f.keyword.as_str()),
            _ => None,
        }).collect();
        assert!(names.contains(&"y"), "selected layer field y present: {:?}", names);
    }

    #[test]
    fn resolver_empty_selection_composes_base_alone() {
        // Layers are OPTIONAL: a pragma naming no layers gets the base
        // schema only (§8.1), whose signature is single-component.
        let layered_src = "\
tel 1.0

name url-layered

document
  field x String

layer
  name extra
  overlay
    field y String optional
";
        let mut fetcher = InMemoryFetcher::new();
        fetcher.add("example.org/url-layered:1.0.0", layered_src);
        let mut r = Resolver::with_fetcher(fetcher);
        let id = SchemaIdentifier::parse("example.org/url-layered:1.0.0").unwrap();
        let s = r.resolve(&id).unwrap();
        let names: Vec<&str> = s.document.members.iter().filter_map(|m| match m {
            Member::Field(f) => Some(f.keyword.as_str()),
            _ => None,
        }).collect();
        assert!(names.contains(&"x"), "base field x present: {:?}", names);
        assert!(!names.contains(&"y"), "unselected layer field y absent: {:?}", names);
    }

    #[test]
    fn resolver_unknown_layer_is_reported() {
        let layered_src = "\
tel 1.0

name url-layered

document
  field x String

layer
  name extra
  overlay
    field y String optional
";
        let mut fetcher = InMemoryFetcher::new();
        fetcher.add("example.org/url-layered:1.0.0", layered_src);
        let mut r = Resolver::with_fetcher(fetcher);
        let id = SchemaIdentifier {
            reference: Some("example.org/url-layered:1.0.0".to_string()),
            layers: vec!["nonexistent".to_string()],
            signature: None,
        };
        let err = r.resolve(&id).unwrap_err();
        assert!(matches!(err, ResolutionError::UnknownLayer { .. }));
    }

    #[test]
    fn layer_order_check_follows_declaration_order() {
        let two_layer_src = "\
tel 1.0

name two-layers

document
  field x String

layer
  name aa
  overlay
    field y String optional

layer
  name bb
  overlay
    field z String optional
";
        let parsed = parse(two_layer_src);
        let a = "aa".to_string();
        let b = "bb".to_string();
        assert!(layers_in_declaration_order(&parsed.document, &[a.clone(), b.clone()]));
        assert!(layers_in_declaration_order(&parsed.document, &[b.clone()]));
        assert!(!layers_in_declaration_order(&parsed.document, &[b, a]));
    }

    #[test]
    fn add_bintel_to_library_round_trip_matches_source_form() {
        // Bytes produced by `schema_to_bintel` for a parsed schema document
        // round-trip through `add_bintel_to_library`, producing the same
        // signature as `add_to_library` for the same TEL source.
        let src = "tel 1.0\n\nname my-schema\n\ndocument\n  field x String\n";
        let mut from_source: Resolver<InMemoryFetcher> = Resolver::new();
        let sig_src = from_source.add_to_library(src).unwrap();

        let parsed = parse(src);
        let bintel_bytes = bintel::schema_to_bintel(&parsed.document);
        let mut from_bintel: Resolver<InMemoryFetcher> = Resolver::new();
        let sig_bintel = from_bintel.add_bintel_to_library(&bintel_bytes).unwrap();

        assert_eq!(sig_src, sig_bintel,
                   "BinTEL load must yield the same signature as source load");
    }

    #[test]
    fn add_bintel_to_library_handles_layered_schema() {
        let layered_src = "\
tel 1.0

name url-layered

document
  field x String

layer
  name extra
  overlay
    field y String optional
";
        let mut from_source: Resolver<InMemoryFetcher> = Resolver::new();
        let sig_src = from_source.add_to_library(layered_src).unwrap();
        assert_eq!(sig_src.len(), 37, "two-component signature is 37 bytes");

        let parsed = parse(layered_src);
        let bintel_bytes = bintel::schema_to_bintel(&parsed.document);
        let mut from_bintel: Resolver<InMemoryFetcher> = Resolver::new();
        let sig_bintel = from_bintel.add_bintel_to_library(&bintel_bytes).unwrap();
        assert_eq!(sig_src, sig_bintel);

        // The loaded schema is now resolvable by its signature.
        let id = SchemaIdentifier { reference: None, layers: vec![], signature: Some(sig_bintel) };
        let s = from_bintel.resolve(&id).expect("layered schema resolves from BinTEL load");
        assert_eq!(s.name, "url-layered");
    }

    #[test]
    fn add_bintel_to_library_rejects_non_tels_signature() {
        // A BinTEL document not signed under tels is not a valid
        // schema-representation; add_bintel_to_library MUST reject it.
        let src = "tel 1.0\n\nname my-schema\n\ndocument\n  field x String\n";
        let parsed = parse(src);
        let composed = construct_schema(&parsed.document);
        // Encode the data using `composed` itself as the schema — a
        // non-tels signature.
        let data_doc = crate::Document {
            interpreter_directive: None, pragma: None,
            line_endings: crate::LineEndings::LF,
            children: Vec::new(),
        };
        let other_hash = bintel::value_hash(&data_doc, &composed);
        let bogus_bytes = bintel::encode_document_with_signature(
            &data_doc, &composed, &[other_hash]);
        let mut r: Resolver<InMemoryFetcher> = Resolver::new();
        let err = r.add_bintel_to_library(&bogus_bytes).unwrap_err();
        assert!(matches!(err, ResolutionError::SignatureMismatch { .. }),
                "expected SignatureMismatch, got: {:?}", err);
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
  field x String

layer
  name extra
  overlay
    field y String
";
        let sig = r.add_to_library(src).unwrap();
        // Now drop the layer from the library and re-attempt resolution.
        let layer_keys: Vec<[u8; 32]> = r.layer_library.keys().copied().collect();
        for k in &layer_keys { r.layer_library.remove(k); }
        let id = SchemaIdentifier { reference: None, layers: vec![], signature: Some(sig) };
        let err = r.resolve(&id).unwrap_err();
        assert!(matches!(err, ResolutionError::NotFound));
    }
}
