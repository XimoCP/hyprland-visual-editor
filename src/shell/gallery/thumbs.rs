// HVE 2 — Real wallpaper thumbnail pipeline (gallery-immersive-redesign
// PR4, design D8).
//
// Decodes a theme's source image, cover-crops + scales it to ≤400×720 and
// caches the result as a PNG under `$XDG_CACHE_HOME/hve/thumbs/<key>.png`,
// keyed by hash(source path + mtime) with LRU 200 prune semantics (spec:
// Real Wallpaper Thumbnails; model.rs R8 cache capacity reused).
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Thumbnail upper bound (design D8 / spec: decode ≤400×720 source size).
pub const THUMB_MAX_W: u32 = 400;
pub const THUMB_MAX_H: u32 = 720;

/// Cache-key contract (task 4.5): deterministic hash of the SOURCE PATH +
/// MTIME so a changed wallpaper invalidates its stale thumbnail. Uses
/// std's SipHash via DefaultHasher — no crypto dependency; collisions are
/// irrelevant at LRU 200 scale.
pub fn cache_key(source: &Path, mtime: SystemTime) -> String {
    unimplemented!("4.6 GREEN")
}

// ── 4.5 RED tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn st(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    #[test]
    fn cache_key_is_stable_for_same_input() {
        let a = cache_key(Path::new("/imgs/wall.png"), st(100));
        let b = cache_key(Path::new("/imgs/wall.png"), st(100));
        assert_eq!(a, b, "same path+mtime must map to the same cache key");
    }

    #[test]
    fn cache_key_differs_by_path() {
        let a = cache_key(Path::new("/imgs/a.png"), st(100));
        let b = cache_key(Path::new("/imgs/b.png"), st(100));
        assert_ne!(a, b, "different sources must not collide");
    }

    #[test]
    fn cache_key_differs_by_mtime() {
        let a = cache_key(Path::new("/imgs/wall.png"), st(100));
        let b = cache_key(Path::new("/imgs/wall.png"), st(200));
        assert_ne!(a, b, "mtime change must invalidate the cached thumb");
    }

    #[test]
    fn cache_key_is_hex_string() {
        let k = cache_key(Path::new("/imgs/wall.png"), st(100));
        assert_eq!(k.len(), 16, "u64 hash rendered as 16 hex chars");
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()), "hex only, got {k}");
    }

    // ── LRU 200 prune semantics (task 4.5; reuses model.rs R8 capacity) ──

    use super::super::model::{ThumbnailCache, THUMBNAIL_CACHE_CAPACITY};

    fn store() -> ThumbnailCache {
        ThumbnailCache::with_capacity(THUMBNAIL_CACHE_CAPACITY)
    }

    #[test]
    fn lru_prunes_to_200_and_evicts_oldest() {
        let mut s = store();
        for i in 0..250 {
            s.insert(cache_key(Path::new(&format!("/i/{i}.png")), st(i)), PathBuf::from(format!("/{i}.png")));
        }
        assert_eq!(s.len(), 200, "LRU bounded at capacity");
        assert!(!s.contains(&cache_key(Path::new("/i/0.png"), st(0))), "oldest evicted");
        assert!(!s.contains(&cache_key(Path::new("/i/49.png"), st(49))), "pre-eviction cutoff gone");
        assert!(s.contains(&cache_key(Path::new("/i/50.png"), st(50))), "cutoff retained");
        assert!(s.contains(&cache_key(Path::new("/i/249.png"), st(249))), "newest retained");
    }
}
