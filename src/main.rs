// SPDX-License-Identifier: MPL-2.0

use anyhow::Result;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub display_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub char_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub display_path: String,
    pub matches: Vec<SearchMatch>,
    pub score: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub results_count: usize,
    pub total_files: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DaemonRequest {
    Search { query: String, limit: Option<usize> },
    Refresh,
    Status,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DaemonResponse {
    SearchResults(SearchResponse),
    RefreshComplete {
        files_count: usize,
    },
    Status {
        files_count: usize,
        last_updated: u64,
    },
    Error {
        message: String,
    },
}

pub struct FileIndex {
    files: Vec<FileEntry>,
    last_updated: std::time::SystemTime,
    matcher: Matcher,
}

impl Default for FileIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl FileIndex {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            last_updated: std::time::SystemTime::now(),
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
        }
    }

    pub fn update(&mut self) -> Result<()> {
        info!("Updating file index...");

        let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());

        let output = Command::new("fd")
            .args([".", &home, "--type", "file"])
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "fd command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8(output.stdout)?;
        self.files = stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|path| {
                let display_path = if path.starts_with(&home) {
                    format!("~{}", &path[home.len()..])
                } else {
                    path.to_string()
                };

                FileEntry {
                    path: path.to_string(),
                    display_path,
                }
            })
            .collect();

        self.last_updated = std::time::SystemTime::now();
        info!("Indexed {} files", self.files.len());
        Ok(())
    }

    pub fn search(&mut self, query: &str, limit: Option<usize>) -> Vec<SearchResult> {
        let limit = limit.unwrap_or(100);

        if query.is_empty() {
            return self
                .files
                .iter()
                .take(limit)
                .map(|file| SearchResult {
                    path: file.path.clone(),
                    display_path: file.display_path.clone(),
                    matches: Vec::new(),
                    score: 0,
                })
                .collect();
        }

        let mut results = Vec::new();

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

        for file in &self.files {
            let filename = Path::new(&file.display_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");

            let mut haystack_vec = Vec::new();
            let haystack = Utf32Str::new(filename, &mut haystack_vec);

            if let Some(score) = pattern.score(haystack, &mut self.matcher) {
                let mut indices = Vec::new();
                pattern.indices(haystack, &mut self.matcher, &mut indices);

                let filename_offset = if let Some(last_slash_pos) = file.display_path.rfind('/') {
                    last_slash_pos + 1
                } else {
                    0
                };

                let matches = indices
                    .into_iter()
                    .map(|idx| SearchMatch {
                        char_index: idx + filename_offset as u32,
                    })
                    .collect();

                results.push(SearchResult {
                    path: file.path.clone(),
                    display_path: file.display_path.clone(),
                    matches,
                    score: score as i32,
                });
            }
        }

        results.sort_by(|a, b| b.score.cmp(&a.score));
        results.truncate(limit);
        results
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn last_updated_timestamp(&self) -> u64 {
        self.last_updated
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

async fn handle_client(
    mut stream: UnixStream,
    file_index: Arc<Mutex<FileIndex>>,
    response_writer: Arc<Mutex<Option<UnixStream>>>,
    active_clients: Arc<AtomicUsize>,
) -> Result<()> {
    active_clients.fetch_add(1, Ordering::Relaxed);
    debug!(
        "Client connected. Active clients: {}",
        active_clients.load(Ordering::Relaxed)
    );
    let (reader, mut fallback_writer) = stream.split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        debug!("Received request: {}", line);

        let response = match serde_json::from_str::<DaemonRequest>(&line) {
            Ok(request) => match request {
                DaemonRequest::Search { query, limit } => {
                    let mut index = file_index.lock().unwrap();
                    let results = index.search(&query, limit);
                    let results_count = results.len();
                    let total_files = index.len();
                    DaemonResponse::SearchResults(SearchResponse {
                        results,
                        results_count,
                        total_files,
                    })
                }
                DaemonRequest::Refresh => {
                    let mut index = file_index.lock().unwrap();
                    match index.update() {
                        Ok(()) => DaemonResponse::RefreshComplete {
                            files_count: index.len(),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                DaemonRequest::Status => {
                    let index = file_index.lock().unwrap();
                    DaemonResponse::Status {
                        files_count: index.len(),
                        last_updated: index.last_updated_timestamp(),
                    }
                }
            },
            Err(e) => DaemonResponse::Error {
                message: format!("Invalid request: {}", e),
            },
        };

        let response_json = serde_json::to_string(&response)?;

        let mut sent_via_response_socket = false;

        let response_writer_option = {
            let mut response_writer_guard = response_writer.lock().unwrap();
            response_writer_guard.take()
        };

        if let Some(mut writer) = response_writer_option {
            let send_result = async {
                writer.write_all(response_json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                Ok::<_, std::io::Error>(writer)
            }
            .await;

            match send_result {
                Ok(writer) => {
                    debug!("Sent response via response socket: {}", response_json);
                    sent_via_response_socket = true;
                    let mut response_writer_guard = response_writer.lock().unwrap();
                    *response_writer_guard = Some(writer);
                }
                Err(e) => {
                    warn!("Failed to send via response socket: {}", e);
                }
            }
        }

        if !sent_via_response_socket {
            match fallback_writer.write_all(response_json.as_bytes()).await {
                Ok(_) => match fallback_writer.write_all(b"\n").await {
                    Ok(_) => match fallback_writer.flush().await {
                        Ok(_) => {
                            debug!(
                                "Sent response via request socket (fallback): {}",
                                response_json
                            );
                        }
                        Err(e) => {
                            warn!("Failed to flush fallback response: {}", e);
                            break;
                        }
                    },
                    Err(e) => {
                        warn!("Failed to write newline to fallback: {}", e);
                        break;
                    }
                },
                Err(e) => {
                    warn!("Failed to write fallback response: {}", e);
                    break;
                }
            }
        }
    }

    active_clients.fetch_sub(1, Ordering::Relaxed);
    debug!(
        "Client disconnected. Active clients: {}",
        active_clients.load(Ordering::Relaxed)
    );
    Ok(())
}

async fn start_socket_server(
    file_index: Arc<Mutex<FileIndex>>,
    response_writer: Arc<Mutex<Option<UnixStream>>>,
    active_clients: Arc<AtomicUsize>,
) -> Result<()> {
    let socket_path = "/tmp/quickfile-daemon.sock";

    if std::path::Path::new(socket_path).exists() {
        std::fs::remove_file(socket_path)?;
    }

    let listener = UnixListener::bind(socket_path)?;
    info!("Request server listening on {}", socket_path);

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let file_index = Arc::clone(&file_index);
                let response_writer = Arc::clone(&response_writer);
                let active_clients = Arc::clone(&active_clients);
                tokio::spawn(async move {
                    if let Err(e) =
                        handle_client(stream, file_index, response_writer, active_clients).await
                    {
                        warn!("Client handler error: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}

async fn manage_response_connection(
    response_writer: Arc<Mutex<Option<UnixStream>>>,
    active_clients: Arc<AtomicUsize>,
) {
    let response_socket_path = "/tmp/quickfile-response.sock";

    loop {
        let has_active_clients = active_clients.load(Ordering::Relaxed) > 0;

        if !has_active_clients {
            {
                let mut writer_guard = response_writer.lock().unwrap();
                if writer_guard.is_some() {
                    debug!("No active clients, disconnecting from response server");
                    *writer_guard = None;
                }
            }
            sleep(Duration::from_millis(1000)).await;
            continue;
        }

        let needs_connection = {
            let writer_guard = response_writer.lock().unwrap();
            writer_guard.is_none()
        };

        if !needs_connection {
            sleep(Duration::from_millis(1000)).await;
            continue;
        }

        info!(
            "Attempting to connect to response server at {} (active clients: {})",
            response_socket_path,
            active_clients.load(Ordering::Relaxed)
        );

        match UnixStream::connect(response_socket_path).await {
            Ok(stream) => {
                info!("Connected to response server");
                {
                    let mut writer_guard = response_writer.lock().unwrap();
                    *writer_guard = Some(stream);
                }

                sleep(Duration::from_millis(5000)).await;
            }
            Err(e) => {
                debug!("Failed to connect to response server: {}", e);
                sleep(Duration::from_millis(2000)).await;
            }
        }
    }
}

async fn periodic_refresh(file_index: Arc<Mutex<FileIndex>>) {
    let mut interval = tokio::time::interval(Duration::from_secs(300));

    loop {
        interval.tick().await;
        info!("Performing periodic file index refresh...");

        let mut index = file_index.lock().unwrap();
        if let Err(e) = index.update() {
            error!("Periodic refresh failed: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("Starting quickfile daemon...");

    let file_index = Arc::new(Mutex::new(FileIndex::new()));

    {
        let mut index = file_index.lock().unwrap();
        if let Err(e) = index.update() {
            error!("Failed to initialize file index: {}", e);
            return Err(e);
        }
    }

    let response_writer = Arc::new(Mutex::new(None));

    let active_clients = Arc::new(AtomicUsize::new(0));

    let refresh_index = Arc::clone(&file_index);
    tokio::spawn(periodic_refresh(refresh_index));

    let response_manager_writer = Arc::clone(&response_writer);
    let response_manager_clients = Arc::clone(&active_clients);
    tokio::spawn(manage_response_connection(
        response_manager_writer,
        response_manager_clients,
    ));

    start_socket_server(file_index, response_writer, active_clients).await?;

    Ok(())
}
