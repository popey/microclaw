use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use tracing::{info, warn};

use crate::mcp::{McpManager, McpServer, McpToolInfo};
use microclaw_core::error::MicroClawError;
use microclaw_storage::db::{call_blocking, Database, Memory};

#[derive(Clone)]
pub struct MemoryMcpClient {
    server: Arc<McpServer>,
    query_tool: String,
    upsert_tool: String,
}

impl MemoryMcpClient {
    pub fn discover(manager: &McpManager) -> Option<Self> {
        let tools = manager.all_tools();
        let mut grouped: HashMap<String, (Option<Arc<McpServer>>, bool, bool)> = HashMap::new();
        for (server, tool) in tools {
            let entry =
                grouped
                    .entry(tool.server_name.clone())
                    .or_insert((Some(server), false, false));
            if tool.name == "memory_query" {
                entry.1 = true;
            }
            if tool.name == "memory_upsert" {
                entry.2 = true;
            }
        }

        for (name, (server_opt, has_query, has_upsert)) in grouped {
            if has_query && has_upsert {
                if let Some(server) = server_opt {
                    info!("Memory MCP backend enabled via server '{name}'");
                    return Some(Self {
                        server,
                        query_tool: "memory_query".to_string(),
                        upsert_tool: "memory_upsert".to_string(),
                    });
                }
            }
        }
        None
    }

    async fn call_query(&self, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let text = self.server.call_tool(&self.query_tool, payload).await?;
        parse_json_loose(&text)
    }

    async fn call_upsert(&self, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let text = self.server.call_tool(&self.upsert_tool, payload).await?;
        parse_json_loose(&text)
    }
}

pub struct MemoryBackend {
    provider: Arc<dyn MemoryProvider>,
}

impl MemoryBackend {
    pub fn new(db: Arc<Database>, mcp: Option<MemoryMcpClient>) -> Self {
        let sqlite: Arc<dyn MemoryProvider> = Arc::new(SqliteMemoryProvider::new(db.clone()));
        let provider: Arc<dyn MemoryProvider> = match mcp {
            Some(mcp_client) => Arc::new(FallbackMemoryProvider::new(
                Arc::new(McpMemoryProvider::new(mcp_client)),
                sqlite,
            )),
            None => sqlite,
        };
        Self { provider }
    }

    pub fn local_only(db: Arc<Database>) -> Self {
        Self {
            provider: Arc::new(SqliteMemoryProvider::new(db)),
        }
    }

    pub fn prefers_mcp(&self) -> bool {
        self.provider.prefers_mcp()
    }

    pub async fn get_all_memories_for_chat(
        &self,
        chat_id: Option<i64>,
    ) -> Result<Vec<Memory>, MicroClawError> {
        self.provider.get_all_memories_for_chat(chat_id).await
    }

    pub async fn get_memories_for_context(
        &self,
        chat_id: i64,
        limit: usize,
    ) -> Result<Vec<Memory>, MicroClawError> {
        self.provider.get_memories_for_context(chat_id, limit).await
    }

    pub async fn search_memories_with_options(
        &self,
        chat_id: i64,
        query: &str,
        limit: usize,
        include_archived: bool,
        broad_recall: bool,
    ) -> Result<Vec<Memory>, MicroClawError> {
        self.provider
            .search_memories_with_options(chat_id, query, limit, include_archived, broad_recall)
            .await
    }

    pub async fn get_memory_by_id(&self, id: i64) -> Result<Option<Memory>, MicroClawError> {
        self.provider.get_memory_by_id(id).await
    }

    pub async fn insert_memory_with_metadata(
        &self,
        chat_id: Option<i64>,
        content: &str,
        category: &str,
        source: &str,
        confidence: f64,
    ) -> Result<i64, MicroClawError> {
        self.provider
            .insert_memory_with_metadata(chat_id, content, category, source, confidence)
            .await
    }

    pub async fn update_memory_with_metadata(
        &self,
        id: i64,
        content: &str,
        category: &str,
        confidence: f64,
        source: &str,
    ) -> Result<bool, MicroClawError> {
        self.provider
            .update_memory_with_metadata(id, content, category, confidence, source)
            .await
    }

    pub async fn update_memory_content(
        &self,
        id: i64,
        content: &str,
        category: &str,
    ) -> Result<bool, MicroClawError> {
        self.update_memory_with_metadata(id, content, category, 0.8, "tool")
            .await
    }

    pub async fn archive_memory(&self, id: i64) -> Result<bool, MicroClawError> {
        self.provider.archive_memory(id).await
    }

    pub async fn supersede_memory(
        &self,
        from_memory_id: i64,
        new_content: &str,
        category: &str,
        source: &str,
        confidence: f64,
        reason: Option<&str>,
    ) -> Result<i64, MicroClawError> {
        self.provider
            .supersede_memory(
                from_memory_id,
                new_content,
                category,
                source,
                confidence,
                reason,
            )
            .await
    }

    pub async fn touch_memory_last_seen(
        &self,
        id: i64,
        confidence_floor: Option<f64>,
    ) -> Result<bool, MicroClawError> {
        self.provider
            .touch_memory_last_seen(id, confidence_floor)
            .await
    }
}

#[async_trait]
pub trait MemoryProvider: Send + Sync {
    fn prefers_mcp(&self) -> bool {
        false
    }

    async fn get_all_memories_for_chat(
        &self,
        chat_id: Option<i64>,
    ) -> Result<Vec<Memory>, MicroClawError>;

    async fn get_memories_for_context(
        &self,
        chat_id: i64,
        limit: usize,
    ) -> Result<Vec<Memory>, MicroClawError>;

    async fn search_memories_with_options(
        &self,
        chat_id: i64,
        query: &str,
        limit: usize,
        include_archived: bool,
        broad_recall: bool,
    ) -> Result<Vec<Memory>, MicroClawError>;

    async fn get_memory_by_id(&self, id: i64) -> Result<Option<Memory>, MicroClawError>;

    async fn insert_memory_with_metadata(
        &self,
        chat_id: Option<i64>,
        content: &str,
        category: &str,
        source: &str,
        confidence: f64,
    ) -> Result<i64, MicroClawError>;

    async fn update_memory_with_metadata(
        &self,
        id: i64,
        content: &str,
        category: &str,
        confidence: f64,
        source: &str,
    ) -> Result<bool, MicroClawError>;

    async fn archive_memory(&self, id: i64) -> Result<bool, MicroClawError>;

    async fn supersede_memory(
        &self,
        from_memory_id: i64,
        new_content: &str,
        category: &str,
        source: &str,
        confidence: f64,
        reason: Option<&str>,
    ) -> Result<i64, MicroClawError>;

    async fn touch_memory_last_seen(
        &self,
        id: i64,
        confidence_floor: Option<f64>,
    ) -> Result<bool, MicroClawError>;
}

struct SqliteMemoryProvider {
    db: Arc<Database>,
}

impl SqliteMemoryProvider {
    fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl MemoryProvider for SqliteMemoryProvider {
    async fn get_all_memories_for_chat(
        &self,
        chat_id: Option<i64>,
    ) -> Result<Vec<Memory>, MicroClawError> {
        let chat = chat_id;
        call_blocking(self.db.clone(), move |db| {
            db.get_all_memories_for_chat(chat)
        })
        .await
    }

    async fn get_memories_for_context(
        &self,
        chat_id: i64,
        limit: usize,
    ) -> Result<Vec<Memory>, MicroClawError> {
        call_blocking(self.db.clone(), move |db| {
            db.get_memories_for_context(chat_id, limit)
        })
        .await
    }

    async fn search_memories_with_options(
        &self,
        chat_id: i64,
        query: &str,
        limit: usize,
        include_archived: bool,
        broad_recall: bool,
    ) -> Result<Vec<Memory>, MicroClawError> {
        let q = query.to_string();
        call_blocking(self.db.clone(), move |db| {
            db.search_memories_with_options(chat_id, &q, limit, include_archived, broad_recall)
        })
        .await
    }

    async fn get_memory_by_id(&self, id: i64) -> Result<Option<Memory>, MicroClawError> {
        call_blocking(self.db.clone(), move |db| db.get_memory_by_id(id)).await
    }

    async fn insert_memory_with_metadata(
        &self,
        chat_id: Option<i64>,
        content: &str,
        category: &str,
        source: &str,
        confidence: f64,
    ) -> Result<i64, MicroClawError> {
        let text = content.to_string();
        let cat = category.to_string();
        let src = source.to_string();
        call_blocking(self.db.clone(), move |db| {
            db.insert_memory_with_metadata(chat_id, &text, &cat, &src, confidence)
        })
        .await
    }

    async fn update_memory_with_metadata(
        &self,
        id: i64,
        content: &str,
        category: &str,
        confidence: f64,
        source: &str,
    ) -> Result<bool, MicroClawError> {
        let text = content.to_string();
        let cat = category.to_string();
        let src = source.to_string();
        call_blocking(self.db.clone(), move |db| {
            db.update_memory_with_metadata(id, &text, &cat, confidence, &src)
        })
        .await
    }

    async fn archive_memory(&self, id: i64) -> Result<bool, MicroClawError> {
        call_blocking(self.db.clone(), move |db| db.archive_memory(id)).await
    }

    async fn supersede_memory(
        &self,
        from_memory_id: i64,
        new_content: &str,
        category: &str,
        source: &str,
        confidence: f64,
        reason: Option<&str>,
    ) -> Result<i64, MicroClawError> {
        let text = new_content.to_string();
        let cat = category.to_string();
        let src = source.to_string();
        let why = reason.map(|value| value.to_string());
        call_blocking(self.db.clone(), move |db| {
            db.supersede_memory(
                from_memory_id,
                &text,
                &cat,
                &src,
                confidence,
                why.as_deref(),
            )
        })
        .await
    }

    async fn touch_memory_last_seen(
        &self,
        id: i64,
        confidence_floor: Option<f64>,
    ) -> Result<bool, MicroClawError> {
        call_blocking(self.db.clone(), move |db| {
            db.touch_memory_last_seen(id, confidence_floor)
        })
        .await
    }
}

struct McpMemoryProvider {
    client: MemoryMcpClient,
}

impl McpMemoryProvider {
    fn new(client: MemoryMcpClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl MemoryProvider for McpMemoryProvider {
    fn prefers_mcp(&self) -> bool {
        true
    }

    async fn get_all_memories_for_chat(
        &self,
        chat_id: Option<i64>,
    ) -> Result<Vec<Memory>, MicroClawError> {
        let payload = serde_json::json!({
            "op": "list",
            "chat_id": chat_id,
        });
        let value = self
            .client
            .call_query(payload)
            .await
            .map_err(MicroClawError::ToolExecution)?;
        parse_memory_list(&value).ok_or_else(|| {
            MicroClawError::ToolExecution(
                "memory_query(list) returned invalid memory payload".to_string(),
            )
        })
    }

    async fn get_memories_for_context(
        &self,
        chat_id: i64,
        limit: usize,
    ) -> Result<Vec<Memory>, MicroClawError> {
        let payload = serde_json::json!({
            "op": "context",
            "chat_id": chat_id,
            "limit": limit,
        });
        let value = self
            .client
            .call_query(payload)
            .await
            .map_err(MicroClawError::ToolExecution)?;
        parse_memory_list(&value).ok_or_else(|| {
            MicroClawError::ToolExecution(
                "memory_query(context) returned invalid memory payload".to_string(),
            )
        })
    }

    async fn search_memories_with_options(
        &self,
        chat_id: i64,
        query: &str,
        limit: usize,
        include_archived: bool,
        broad_recall: bool,
    ) -> Result<Vec<Memory>, MicroClawError> {
        let payload = serde_json::json!({
            "op": "search",
            "chat_id": chat_id,
            "query": query,
            "limit": limit,
            "include_archived": include_archived,
            "broad_recall": broad_recall,
        });
        let value = self
            .client
            .call_query(payload)
            .await
            .map_err(MicroClawError::ToolExecution)?;
        parse_memory_list(&value).ok_or_else(|| {
            MicroClawError::ToolExecution(
                "memory_query(search) returned invalid memory payload".to_string(),
            )
        })
    }

    async fn get_memory_by_id(&self, id: i64) -> Result<Option<Memory>, MicroClawError> {
        let payload = serde_json::json!({
            "op": "get",
            "id": id,
        });
        let value = self
            .client
            .call_query(payload)
            .await
            .map_err(MicroClawError::ToolExecution)?;
        if let Some(memories) = parse_memory_list(&value) {
            return Ok(memories.into_iter().next());
        }
        if let Some(memory) = parse_single_memory(&value) {
            return Ok(Some(memory));
        }
        Err(MicroClawError::ToolExecution(
            "memory_query(get) returned invalid memory payload".to_string(),
        ))
    }

    async fn insert_memory_with_metadata(
        &self,
        chat_id: Option<i64>,
        content: &str,
        category: &str,
        source: &str,
        confidence: f64,
    ) -> Result<i64, MicroClawError> {
        let payload = serde_json::json!({
            "op": "insert",
            "chat_id": chat_id,
            "content": content,
            "category": category,
            "source": source,
            "confidence": confidence,
        });
        let value = self
            .client
            .call_upsert(payload)
            .await
            .map_err(MicroClawError::ToolExecution)?;
        extract_id(&value).ok_or_else(|| {
            MicroClawError::ToolExecution(
                "memory_upsert(insert) returned invalid memory payload".to_string(),
            )
        })
    }

    async fn update_memory_with_metadata(
        &self,
        id: i64,
        content: &str,
        category: &str,
        confidence: f64,
        source: &str,
    ) -> Result<bool, MicroClawError> {
        let payload = serde_json::json!({
            "op": "update",
            "id": id,
            "content": content,
            "category": category,
            "source": source,
            "confidence": confidence,
        });
        let value = self
            .client
            .call_upsert(payload)
            .await
            .map_err(MicroClawError::ToolExecution)?;
        extract_bool_flag(&value).ok_or_else(|| {
            MicroClawError::ToolExecution(
                "memory_upsert(update) returned invalid memory payload".to_string(),
            )
        })
    }

    async fn archive_memory(&self, id: i64) -> Result<bool, MicroClawError> {
        let payload = serde_json::json!({
            "op": "archive",
            "id": id,
        });
        let value = self
            .client
            .call_upsert(payload)
            .await
            .map_err(MicroClawError::ToolExecution)?;
        extract_bool_flag(&value).ok_or_else(|| {
            MicroClawError::ToolExecution(
                "memory_upsert(archive) returned invalid memory payload".to_string(),
            )
        })
    }

    async fn supersede_memory(
        &self,
        from_memory_id: i64,
        new_content: &str,
        category: &str,
        source: &str,
        confidence: f64,
        reason: Option<&str>,
    ) -> Result<i64, MicroClawError> {
        let payload = serde_json::json!({
            "op": "supersede",
            "from_memory_id": from_memory_id,
            "content": new_content,
            "category": category,
            "source": source,
            "confidence": confidence,
            "reason": reason,
        });
        let value = self
            .client
            .call_upsert(payload)
            .await
            .map_err(MicroClawError::ToolExecution)?;
        extract_id(&value).ok_or_else(|| {
            MicroClawError::ToolExecution(
                "memory_upsert(supersede) returned invalid memory payload".to_string(),
            )
        })
    }

    async fn touch_memory_last_seen(
        &self,
        id: i64,
        confidence_floor: Option<f64>,
    ) -> Result<bool, MicroClawError> {
        let payload = serde_json::json!({
            "op": "touch",
            "id": id,
            "confidence_floor": confidence_floor,
        });
        let value = self
            .client
            .call_upsert(payload)
            .await
            .map_err(MicroClawError::ToolExecution)?;
        extract_bool_flag(&value).ok_or_else(|| {
            MicroClawError::ToolExecution(
                "memory_upsert(touch) returned invalid memory payload".to_string(),
            )
        })
    }
}

struct FallbackMemoryProvider {
    primary: Arc<dyn MemoryProvider>,
    fallback: Arc<dyn MemoryProvider>,
}

impl FallbackMemoryProvider {
    fn new(primary: Arc<dyn MemoryProvider>, fallback: Arc<dyn MemoryProvider>) -> Self {
        Self { primary, fallback }
    }

    async fn fallback_on_err<T, FutPrimary, FutFallback>(
        &self,
        op_name: &str,
        primary: FutPrimary,
        fallback: FutFallback,
    ) -> Result<T, MicroClawError>
    where
        FutPrimary: std::future::Future<Output = Result<T, MicroClawError>>,
        FutFallback: std::future::Future<Output = Result<T, MicroClawError>>,
    {
        match primary.await {
            Ok(value) => Ok(value),
            Err(err) => {
                warn!("{op_name} failed via primary memory provider ({err}); falling back");
                fallback.await
            }
        }
    }
}

#[async_trait]
impl MemoryProvider for FallbackMemoryProvider {
    fn prefers_mcp(&self) -> bool {
        self.primary.prefers_mcp()
    }

    async fn get_all_memories_for_chat(
        &self,
        chat_id: Option<i64>,
    ) -> Result<Vec<Memory>, MicroClawError> {
        self.fallback_on_err(
            "memory_query(list)",
            self.primary.get_all_memories_for_chat(chat_id),
            self.fallback.get_all_memories_for_chat(chat_id),
        )
        .await
    }

    async fn get_memories_for_context(
        &self,
        chat_id: i64,
        limit: usize,
    ) -> Result<Vec<Memory>, MicroClawError> {
        self.fallback_on_err(
            "memory_query(context)",
            self.primary.get_memories_for_context(chat_id, limit),
            self.fallback.get_memories_for_context(chat_id, limit),
        )
        .await
    }

    async fn search_memories_with_options(
        &self,
        chat_id: i64,
        query: &str,
        limit: usize,
        include_archived: bool,
        broad_recall: bool,
    ) -> Result<Vec<Memory>, MicroClawError> {
        self.fallback_on_err(
            "memory_query(search)",
            self.primary.search_memories_with_options(
                chat_id,
                query,
                limit,
                include_archived,
                broad_recall,
            ),
            self.fallback.search_memories_with_options(
                chat_id,
                query,
                limit,
                include_archived,
                broad_recall,
            ),
        )
        .await
    }

    async fn get_memory_by_id(&self, id: i64) -> Result<Option<Memory>, MicroClawError> {
        self.fallback_on_err(
            "memory_query(get)",
            self.primary.get_memory_by_id(id),
            self.fallback.get_memory_by_id(id),
        )
        .await
    }

    async fn insert_memory_with_metadata(
        &self,
        chat_id: Option<i64>,
        content: &str,
        category: &str,
        source: &str,
        confidence: f64,
    ) -> Result<i64, MicroClawError> {
        self.fallback_on_err(
            "memory_upsert(insert)",
            self.primary
                .insert_memory_with_metadata(chat_id, content, category, source, confidence),
            self.fallback
                .insert_memory_with_metadata(chat_id, content, category, source, confidence),
        )
        .await
    }

    async fn update_memory_with_metadata(
        &self,
        id: i64,
        content: &str,
        category: &str,
        confidence: f64,
        source: &str,
    ) -> Result<bool, MicroClawError> {
        self.fallback_on_err(
            "memory_upsert(update)",
            self.primary
                .update_memory_with_metadata(id, content, category, confidence, source),
            self.fallback
                .update_memory_with_metadata(id, content, category, confidence, source),
        )
        .await
    }

    async fn archive_memory(&self, id: i64) -> Result<bool, MicroClawError> {
        self.fallback_on_err(
            "memory_upsert(archive)",
            self.primary.archive_memory(id),
            self.fallback.archive_memory(id),
        )
        .await
    }

    async fn supersede_memory(
        &self,
        from_memory_id: i64,
        new_content: &str,
        category: &str,
        source: &str,
        confidence: f64,
        reason: Option<&str>,
    ) -> Result<i64, MicroClawError> {
        self.fallback_on_err(
            "memory_upsert(supersede)",
            self.primary.supersede_memory(
                from_memory_id,
                new_content,
                category,
                source,
                confidence,
                reason,
            ),
            self.fallback.supersede_memory(
                from_memory_id,
                new_content,
                category,
                source,
                confidence,
                reason,
            ),
        )
        .await
    }

    async fn touch_memory_last_seen(
        &self,
        id: i64,
        confidence_floor: Option<f64>,
    ) -> Result<bool, MicroClawError> {
        self.fallback_on_err(
            "memory_upsert(touch)",
            self.primary.touch_memory_last_seen(id, confidence_floor),
            self.fallback.touch_memory_last_seen(id, confidence_floor),
        )
        .await
    }
}

fn parse_json_loose(text: &str) -> Result<serde_json::Value, String> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        return Ok(v);
    }
    for (open, close) in [(b'[', b']'), (b'{', b'}')] {
        if let Some(start) = text.as_bytes().iter().position(|b| *b == open) {
            if let Some(end) = text.as_bytes().iter().rposition(|b| *b == close) {
                if start < end {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text[start..=end]) {
                        return Ok(v);
                    }
                }
            }
        }
    }
    Err("MCP memory response is not valid JSON".to_string())
}

fn parse_memory_list(value: &serde_json::Value) -> Option<Vec<Memory>> {
    if let Some(arr) = value.as_array() {
        return Some(arr.iter().filter_map(parse_single_memory).collect());
    }
    let obj = value.as_object()?;
    if let Some(arr) = obj.get("memories").and_then(|v| v.as_array()) {
        return Some(arr.iter().filter_map(parse_single_memory).collect());
    }
    if let Some(arr) = obj.get("items").and_then(|v| v.as_array()) {
        return Some(arr.iter().filter_map(parse_single_memory).collect());
    }
    None
}

fn parse_single_memory(value: &serde_json::Value) -> Option<Memory> {
    let obj = value.as_object()?;
    let id = obj.get("id").and_then(|v| v.as_i64())?;
    let content = obj.get("content").and_then(|v| v.as_str())?.to_string();
    let category = obj
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("KNOWLEDGE")
        .to_string();
    let now = chrono::Utc::now().to_rfc3339();
    Some(Memory {
        id,
        chat_id: obj.get("chat_id").and_then(|v| v.as_i64()),
        content,
        category,
        created_at: obj
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or(&now)
            .to_string(),
        updated_at: obj
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or(&now)
            .to_string(),
        embedding_model: obj
            .get("embedding_model")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string()),
        confidence: obj
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.8),
        source: obj
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("mcp_memory")
            .to_string(),
        last_seen_at: obj
            .get("last_seen_at")
            .and_then(|v| v.as_str())
            .unwrap_or(&now)
            .to_string(),
        is_archived: obj
            .get("is_archived")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        archived_at: obj
            .get("archived_at")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string()),
    })
}

fn extract_id(value: &serde_json::Value) -> Option<i64> {
    value
        .get("id")
        .and_then(|v| v.as_i64())
        .or_else(|| value.get("memory_id").and_then(|v| v.as_i64()))
        .or_else(|| {
            value
                .get("memory")
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_i64())
        })
}

fn extract_bool_flag(value: &serde_json::Value) -> Option<bool> {
    value
        .get("updated")
        .and_then(|v| v.as_bool())
        .or_else(|| value.get("ok").and_then(|v| v.as_bool()))
        .or_else(|| value.get("success").and_then(|v| v.as_bool()))
}

#[allow(dead_code)]
fn _extract_tool_info(tools: &[McpToolInfo]) -> Vec<String> {
    tools.iter().map(|t| t.name.clone()).collect()
}
