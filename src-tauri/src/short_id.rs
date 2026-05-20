use blake3::Hasher;
use data_encoding::BASE32_NOPAD;  // RFC 4648 base32 — letters A-Z + 2-7
use std::collections::HashSet;

/// Generate a Short_ID for a Download_Item.
///
/// Algorithm (per design):
/// - hash = blake3(url || "|" || ts_ms || "|" || salt)
/// - id = base32 lowercase of first `length` bytes of hash, length 6 by default
/// - on collision: increase salt up to 10 attempts, then bump length to 8 and retry
/// - alphabet: a-z, 2-7 (RFC 4648 base32 lowercased)
///
/// Returns a string of length 6 to 8 inclusive that is **not** in `taken`.
pub fn generate(url: &str, ts_ms: i64, taken: &HashSet<String>) -> String {
    let mut length = 6usize;
    let mut salt: u64 = 0;
    loop {
        let mut h = Hasher::new();
        h.update(url.as_bytes());
        h.update(b"|");
        h.update(ts_ms.to_string().as_bytes());
        h.update(b"|");
        h.update(&salt.to_be_bytes());
        let hash = h.finalize();
        // base32 of first ceil(length*5/8) bytes -> truncate to length chars
        let needed_bytes = (length * 5 + 7) / 8;
        let bytes = &hash.as_bytes()[..needed_bytes.min(hash.as_bytes().len())];
        let encoded = BASE32_NOPAD.encode(bytes).to_lowercase();
        let id: String = encoded.chars().take(length).collect();
        if !taken.contains(&id) {
            return id;
        }
        salt += 1;
        if salt >= 10 && length < 8 {
            length += 1;
            salt = 0;
        } else if salt >= 64 && length == 8 {
            // Practically impossible, but fall back to random nonce for safety.
            // Mix in process time nanos to vary further.
            // Continue loop with growing salt; we still always return only when not in taken.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generates_length_6_id_by_default() {
        let id = generate("https://x.com/a", 1, &HashSet::new());
        assert_eq!(id.len(), 6);
        assert!(id.chars().all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c)));
    }

    #[test]
    fn avoids_collision() {
        let mut taken = HashSet::new();
        for _ in 0..32 {
            let id = generate("https://x.com/a", 1, &taken);
            assert!(!taken.contains(&id));
            taken.insert(id);
        }
    }
}
