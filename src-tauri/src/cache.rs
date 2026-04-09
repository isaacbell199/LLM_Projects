//! Cache system for NexaStory
//!
//! Provides caching capabilities for:
//! - Generated content (LLM responses)
//! - Database query results
//! - Session state

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Cache Types
// ============================================================================

/// Types of cache entries
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CacheType {
    /// LLM generated content
    Generation,
    /// Database query results
    DbQuery,
    /// Embeddings or vectors
    Embedding,
    /// Application session data
    Session,
}

/// A single cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Unique identifier for this entry
    pub id: String,
    /// Type of cache
    pub cache_type: CacheType,
    /// The cached content (JSON string)
    pub content: String,
    /// Hash of the input that produced this result
    pub input_hash: String,
    /// Creation timestamp (Unix epoch seconds)
    pub created_at: u64,
    /// Last access timestamp
    pub last_accessed: u64,
    /// Access count
    pub access_count: u64,
    /// Size in bytes
    pub size_bytes: u64,
    /// Time-to-live in seconds (0 = no expiry)
    pub ttl_seconds: u64,
    /// Associated project ID (if any)
    pub project_id: Option<String>,
    /// Metadata tags
    pub tags: Vec<String>,
}

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    /// Total entries count
    pub total_entries: u64,
    /// Total size in bytes
    pub total_size_bytes: u64,
    /// Entries by type
    pub entries_by_type: HashMap<String, u64>,
    /// Size by type in bytes
    pub size_by_type: HashMap<String, u64>,
    /// Hit count
    pub hit_count: u64,
    /// Miss count
    pub miss_count: u64,
    /// Cache directory path
    pub cache_directory: String,
}

/// Information about a single cache entry for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntryInfo {
    pub id: String,
    pub cache_type: String,
    pub created_at: String,
    pub last_accessed: String,
    pub access_count: u64,
    pub size_bytes: u64,
    pub project_id: Option<String>,
    pub tags: Vec<String>,
}

// ============================================================================
// Cache Directory Management
// ============================================================================

/// Get the cache directory path
fn get_cache_directory() -> PathBuf {
    std::env::var("NEXASTORY_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let exe_dir = std::env::current_exe()
                .expect("Failed to get executable path")
                .parent()
                .expect("Failed to get parent directory")
                .to_path_buf();
            exe_dir.join("data").join("cache")
        })
}

/// Ensure cache subdirectory exists
fn ensure_cache_subdir(cache_type: &CacheType) -> PathBuf {
    let cache_dir = get_cache_directory();
    let subdir = match cache_type {
        CacheType::Generation => "generations",
        CacheType::DbQuery => "db_queries",
        CacheType::Embedding => "embeddings",
        CacheType::Session => "sessions",
    };
    let path = cache_dir.join(subdir);
    fs::create_dir_all(&path).ok();
    path
}

/// Get current Unix timestamp in seconds
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

/// Simple hash function for cache keys
fn simple_hash(input: &str) -> String {
    // Simple FNV-1a hash for cache key generation
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:x}", hash)
}

// ============================================================================
// Cache Operations
// ============================================================================

/// Store an entry in the cache
pub fn cache_store(
    cache_type: CacheType,
    id: &str,
    content: &str,
    input_hash: &str,
    ttl_seconds: u64,
    project_id: Option<String>,
    tags: Vec<String>,
) -> Result<CacheEntry> {
    let dir = ensure_cache_subdir(&cache_type);
    let now = current_timestamp();

    let entry = CacheEntry {
        id: id.to_string(),
        cache_type: cache_type.clone(),
        content: content.to_string(),
        input_hash: input_hash.to_string(),
        created_at: now,
        last_accessed: now,
        access_count: 1,
        size_bytes: content.len() as u64,
        ttl_seconds,
        project_id,
        tags,
    };

    // Save to file
    let filename = format!("{}.json", simple_hash(id));
    let filepath = dir.join(&filename);
    let json = serde_json::to_string_pretty(&entry)?;
    fs::write(&filepath, json)?;

    log::info!("Cache stored: {} ({:?})", id, cache_type);
    Ok(entry)
}

/// Retrieve an entry from the cache
pub fn cache_get(cache_type: CacheType, id: &str) -> Result<Option<CacheEntry>> {
    let dir = ensure_cache_subdir(&cache_type);
    let filename = format!("{}.json", simple_hash(id));
    let filepath = dir.join(&filename);

    if !filepath.exists() {
        return Ok(None);
    }

    let json = fs::read_to_string(&filepath)?;
    let mut entry: CacheEntry = serde_json::from_str(&json)?;

    // Check TTL
    if entry.ttl_seconds > 0 {
        let now = current_timestamp();
        if now > entry.created_at + entry.ttl_seconds {
            // Expired, remove it
            fs::remove_file(&filepath).ok();
            log::info!("Cache expired and removed: {}", id);
            return Ok(None);
        }
    }

    // Update access stats
    entry.last_accessed = current_timestamp();
    entry.access_count += 1;

    // Save updated entry
    let updated_json = serde_json::to_string(&entry)?;
    fs::write(&filepath, updated_json)?;

    Ok(Some(entry))
}

/// Check if entry exists and is valid
pub fn cache_exists(cache_type: CacheType, id: &str) -> bool {
    let dir = ensure_cache_subdir(&cache_type);
    let filename = format!("{}.json", simple_hash(id));
    let filepath = dir.join(&filename);

    if !filepath.exists() {
        return false;
    }

    // Verify it's not expired
    if let Ok(json) = fs::read_to_string(&filepath) {
        if let Ok(entry) = serde_json::from_str::<CacheEntry>(&json) {
            if entry.ttl_seconds > 0 {
                let now = current_timestamp();
                return now <= entry.created_at + entry.ttl_seconds;
            }
            return true;
        }
    }

    false
}

/// Remove an entry from the cache
pub fn cache_remove(cache_type: CacheType, id: &str) -> Result<bool> {
    let dir = ensure_cache_subdir(&cache_type);
    let filename = format!("{}.json", simple_hash(id));
    let filepath = dir.join(&filename);

    if filepath.exists() {
        fs::remove_file(&filepath)?;
        log::info!("Cache removed: {}", id);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Clear all entries of a specific type
pub fn cache_clear_type(cache_type: CacheType) -> Result<u64> {
    let dir = ensure_cache_subdir(&cache_type);
    let mut count = 0;

    if dir.exists() {
        for entry in fs::read_dir(&dir)? {
            if let Ok(entry) = entry {
                if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                    fs::remove_file(entry.path())?;
                    count += 1;
                }
            }
        }
    }

    log::info!("Cache cleared for type {:?}: {} entries", cache_type, count);
    Ok(count)
}

/// Clear all cache entries
pub fn cache_clear_all() -> Result<u64> {
    let mut total = 0;
    total += cache_clear_type(CacheType::Generation)?;
    total += cache_clear_type(CacheType::DbQuery)?;
    total += cache_clear_type(CacheType::Embedding)?;
    total += cache_clear_type(CacheType::Session)?;
    Ok(total)
}

/// Clean up expired entries across all cache types
pub fn cache_cleanup_expired() -> Result<u64> {
    let mut count = 0;
    let now = current_timestamp();

    for cache_type in [CacheType::Generation, CacheType::DbQuery, CacheType::Embedding, CacheType::Session] {
        let dir = ensure_cache_subdir(&cache_type);

        if dir.exists() {
            for entry in fs::read_dir(&dir)? {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.extension().map(|e| e == "json").unwrap_or(false) {
                        if let Ok(json) = fs::read_to_string(&path) {
                            if let Ok(cache_entry) = serde_json::from_str::<CacheEntry>(&json) {
                                if cache_entry.ttl_seconds > 0 && now > cache_entry.created_at + cache_entry.ttl_seconds {
                                    fs::remove_file(&path)?;
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    log::info!("Cache cleanup: {} expired entries removed", count);
    Ok(count)
}

/// Get cache statistics
pub fn cache_get_stats() -> Result<CacheStats> {
    let mut stats = CacheStats {
        total_entries: 0,
        total_size_bytes: 0,
        entries_by_type: HashMap::new(),
        size_by_type: HashMap::new(),
        hit_count: 0,
        miss_count: 0,
        cache_directory: get_cache_directory().to_string_lossy().to_string(),
    };

    for cache_type in [CacheType::Generation, CacheType::DbQuery, CacheType::Embedding, CacheType::Session] {
        let dir = ensure_cache_subdir(&cache_type);
        let type_name = format!("{:?}", cache_type).to_lowercase();
        let mut type_count = 0u64;
        let mut type_size = 0u64;

        if dir.exists() {
            for entry in fs::read_dir(&dir)? {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.extension().map(|e| e == "json").unwrap_or(false) {
                        if let Ok(metadata) = entry.metadata() {
                            type_count += 1;
                            type_size += metadata.len();
                        }
                    }
                }
            }
        }

        stats.total_entries += type_count;
        stats.total_size_bytes += type_size;
        stats.entries_by_type.insert(type_name.clone(), type_count);
        stats.size_by_type.insert(type_name, type_size);
    }

    // Load hit/miss stats from a metadata file
    let stats_file = get_cache_directory().join("stats.json");
    if stats_file.exists() {
        if let Ok(json) = fs::read_to_string(&stats_file) {
            if let Ok(saved) = serde_json::from_str::<HashMap<String, u64>>(&json) {
                stats.hit_count = saved.get("hit_count").copied().unwrap_or(0);
                stats.miss_count = saved.get("miss_count").copied().unwrap_or(0);
            }
        }
    }

    Ok(stats)
}

/// List entries of a specific type
pub fn cache_list(cache_type: CacheType) -> Result<Vec<CacheEntryInfo>> {
    let dir = ensure_cache_subdir(&cache_type);
    let mut entries = Vec::new();

    if dir.exists() {
        for entry in fs::read_dir(&dir)? {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(json) = fs::read_to_string(&path) {
                        if let Ok(cache_entry) = serde_json::from_str::<CacheEntry>(&json) {
                            entries.push(CacheEntryInfo {
                                id: cache_entry.id,
                                cache_type: format!("{:?}", cache_entry.cache_type).to_lowercase(),
                                created_at: chrono::DateTime::from_timestamp(cache_entry.created_at as i64, 0)
                                    .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                                    .unwrap_or_default(),
                                last_accessed: chrono::DateTime::from_timestamp(cache_entry.last_accessed as i64, 0)
                                    .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                                    .unwrap_or_default(),
                                access_count: cache_entry.access_count,
                                size_bytes: cache_entry.size_bytes,
                                project_id: cache_entry.project_id,
                                tags: cache_entry.tags,
                            });
                        }
                    }
                }
            }
        }
    }

    // Sort by last accessed (most recent first)
    entries.sort_by(|a, b| b.last_accessed.cmp(&a.last_accessed));

    Ok(entries)
}

/// Store a generated content (convenience function)
pub fn cache_generation(
    prompt_hash: &str,
    generated_text: &str,
    project_id: Option<String>,
    model_name: Option<String>,
) -> Result<CacheEntry> {
    let mut tags = vec!["llm".to_string()];
    if let Some(model) = model_name {
        tags.push(model);
    }

    cache_store(
        CacheType::Generation,
        &format!("gen_{}", prompt_hash),
        generated_text,
        prompt_hash,
        86400 * 7, // 7 days TTL for generations
        project_id,
        tags,
    )
}

/// Find cached generation by input hash
pub fn find_cached_generation(input_hash: &str) -> Result<Option<CacheEntry>> {
    let id = format!("gen_{}", input_hash);
    cache_get(CacheType::Generation, &id)
}

/// Store a DB query result
pub fn cache_db_query(
    query_hash: &str,
    result_json: &str,
    ttl_seconds: u64,
) -> Result<CacheEntry> {
    cache_store(
        CacheType::DbQuery,
        &format!("dbq_{}", query_hash),
        result_json,
        query_hash,
        ttl_seconds,
        None,
        vec!["db_query".to_string()],
    )
}

/// Find cached DB query
pub fn find_cached_db_query(query_hash: &str) -> Result<Option<CacheEntry>> {
    let id = format!("dbq_{}", query_hash);
    cache_get(CacheType::DbQuery, &id)
}

/// Store an embedding
pub fn cache_embedding(
    text_hash: &str,
    embedding_json: &str,
    model_name: &str,
) -> Result<CacheEntry> {
    cache_store(
        CacheType::Embedding,
        &format!("emb_{}", text_hash),
        embedding_json,
        text_hash,
        86400 * 30, // 30 days TTL for embeddings
        None,
        vec!["embedding".to_string(), model_name.to_string()],
    )
}

/// Find cached embedding
pub fn find_cached_embedding(text_hash: &str) -> Result<Option<CacheEntry>> {
    let id = format!("emb_{}", text_hash);
    cache_get(CacheType::Embedding, &id)
}

/// Get the cache directory path (for frontend)
pub fn get_cache_directory_path() -> String {
    get_cache_directory().to_string_lossy().to_string()
}

/// Get cache size summary
pub fn get_cache_size() -> Result<(u64, u64)> {
    let stats = cache_get_stats()?;
    Ok((stats.total_entries, stats.total_size_bytes))
}
