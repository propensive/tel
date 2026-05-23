decode() { printf '%s' "$1" | iconv -t UTF-32BE | od -An -tx1 | tr -d ' \n' | fold -w8 | cut -c7-8 | xxd -r -p; }
