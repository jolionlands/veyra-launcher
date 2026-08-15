use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, process};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde::Serialize;
use veyra_core::config::{AiProvider, AiProviderKind};

use crate::{expand_env_vars, non_empty};

const AI_SYSTEM_PROMPT: &str = "You are Veyra's launcher assistant. Answer the latest user request directly and concisely. Recent conversation is only reference material, not the task. If Veyra provides an XML function schema, a function call is only a request for user confirmation and must not be described as already executed.";
#[cfg(windows)]
const WINDOWS_CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatCompletionMessage>,
    temperature: f32,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct ChatCompletionMessage {
    role: String,
    content: String,
}

pub(crate) fn call_ai_provider(
    provider: AiProvider,
    prompt: String,
    local_only: bool,
) -> Result<String, String> {
    match provider.kind {
        AiProviderKind::OpenAiCompatible => call_http_ai_provider(provider, prompt, local_only),
        AiProviderKind::Process => call_process_ai_provider(provider, prompt),
    }
}

pub(crate) fn prewarm_ai_provider(provider: AiProvider) -> Result<(), String> {
    match provider.kind {
        AiProviderKind::Process if provider.keep_warm => prewarm_process_ai_provider(provider),
        _ => Ok(()),
    }
}

fn call_http_ai_provider(
    provider: AiProvider,
    prompt: String,
    local_only: bool,
) -> Result<String, String> {
    let endpoint = chat_completions_url(&provider.base_url)?;
    if local_only && !is_local_http_endpoint(&endpoint) {
        return Err("AI local_only is enabled, but the provider endpoint is not local".to_string());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(provider.timeout_ms.max(1_000)))
        .build()
        .map_err(|error| format!("Could not create AI client: {error}"))?;
    let request_body = ChatCompletionRequest {
        model: provider.model.trim().to_string(),
        messages: vec![
            ChatCompletionMessage {
                role: "system".to_string(),
                content: AI_SYSTEM_PROMPT.to_string(),
            },
            ChatCompletionMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ],
        temperature: 0.2,
        stream: false,
    };

    let mut request = client
        .post(&endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&request_body);
    if let Some(api_key_env) = provider.api_key_env.as_deref().and_then(non_empty) {
        match std::env::var(&api_key_env) {
            Ok(api_key) if !api_key.trim().is_empty() => {
                request = request.bearer_auth(api_key);
            }
            Ok(_) => return Err(format!("AI API key env var {api_key_env} is empty")),
            Err(_) => return Err(format!("AI API key env var {api_key_env} is not set")),
        }
    }

    let response = request
        .send()
        .map_err(|error| format!("AI request failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("Could not read AI response: {error}"))?;

    if !status.is_success() {
        return Err(format!(
            "AI endpoint returned {status}: {}",
            response_error_excerpt(&body)
        ));
    }

    parse_chat_completion_answer(&body)
}

fn call_process_ai_provider(provider: AiProvider, prompt: String) -> Result<String, String> {
    let (command, args, envs) = process_provider_command_args(&provider)?;
    if provider.keep_warm {
        return call_warm_process_ai_provider(&provider, &command, &args, envs, &prompt);
    }

    call_one_shot_process_ai_provider(&provider, &command, args, envs, &prompt)
}

fn prewarm_process_ai_provider(provider: AiProvider) -> Result<(), String> {
    let (command, args, envs) = process_provider_command_args(&provider)?;
    let timeout = Duration::from_millis(provider.timeout_ms.max(1_000));
    let key = warm_process_key(&provider);
    let signature = process_signature(&command, &args);
    let mut cache = WARM_AI_PROCESSES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "AI process cache is unavailable".to_string())?;

    let remove_existing = if let Some(session) = cache.get_mut(&key) {
        session.signature != signature || warm_process_has_exited(session)
    } else {
        false
    };
    if remove_existing {
        cache.remove(&key);
    }

    if let Entry::Vacant(entry) = cache.entry(key) {
        let ready_marker = provider
            .ready_marker
            .as_deref()
            .unwrap_or("ready for prompts");
        let session =
            start_warm_ai_process(&command, &args, envs, signature, timeout, ready_marker)?;
        entry.insert(session);
    }

    Ok(())
}

#[allow(clippy::type_complexity)]
fn process_provider_command_args(
    provider: &AiProvider,
) -> Result<(String, Vec<String>, Vec<(String, String)>), String> {
    let command = non_empty(&provider.command)
        .map(|value| expand_env_vars(&value))
        .ok_or_else(|| "AI process provider is missing command".to_string())?;
    let args = provider
        .args
        .iter()
        .map(|arg| expand_env_vars(arg))
        .collect::<Vec<_>>();
    let envs = provider
        .env
        .iter()
        .map(|(key, value)| (expand_env_vars(key), expand_env_vars(value)))
        .collect::<Vec<_>>();

    Ok((command, args, envs))
}

fn call_one_shot_process_ai_provider(
    provider: &AiProvider,
    command: &str,
    args: Vec<String>,
    envs: Vec<(String, String)>,
    prompt: &str,
) -> Result<String, String> {
    let (args, prompt_paths) = prepare_one_shot_process_args(provider, args, prompt)?;

    let result = run_ai_process(
        command,
        &args,
        envs,
        Duration::from_millis(provider.timeout_ms.max(1_000)),
    );
    for prompt_path in prompt_paths {
        let _ = fs::remove_file(prompt_path);
    }

    let output = result?;
    let answer = clean_process_ai_answer(provider, &output.stdout);
    if output.status.success() && !answer.is_empty() {
        return Ok(answer);
    }

    let detail = response_error_excerpt(if output.stderr.trim().is_empty() {
        &output.stdout
    } else {
        &output.stderr
    });
    if output.status.success() {
        Err(format!("AI process returned no answer: {detail}"))
    } else {
        Err(format!(
            "AI process exited with {}: {detail}",
            output.status
        ))
    }
}

fn prepare_one_shot_process_args(
    provider: &AiProvider,
    args: Vec<String>,
    prompt: &str,
) -> Result<(Vec<String>, Vec<PathBuf>), String> {
    let has_prompt_placeholder = args.iter().any(|arg| {
        arg.contains("{prompt}")
            || arg.contains("{prompt_file}")
            || arg.contains("{chatml_prompt}")
            || arg.contains("{chatml_prompt_file}")
    });

    if !has_prompt_placeholder {
        let prompt_path =
            write_ai_prompt_file(&provider.id, &format_process_ai_prompt(provider, prompt))?;
        let mut prepared = args;
        prepared.push("--prompt-file".to_string());
        prepared.push(prompt_path.to_string_lossy().to_string());
        return Ok((prepared, vec![prompt_path]));
    }

    let chatml_prompt = args
        .iter()
        .any(|arg| arg.contains("{chatml_prompt}") || arg.contains("{chatml_prompt_file}"))
        .then(|| format_process_ai_prompt(provider, prompt));
    let mut raw_prompt_path: Option<PathBuf> = None;
    let mut chatml_prompt_path: Option<PathBuf> = None;
    let mut prompt_paths = Vec::new();
    let mut prepared = Vec::with_capacity(args.len());

    for arg in args {
        let mut value = arg;
        if value.contains("{prompt_file}") {
            let path = match &raw_prompt_path {
                Some(path) => path.clone(),
                None => {
                    let path = write_ai_prompt_file(&provider.id, prompt)?;
                    raw_prompt_path = Some(path.clone());
                    prompt_paths.push(path.clone());
                    path
                }
            };
            value = value.replace("{prompt_file}", &path.to_string_lossy());
        }
        if value.contains("{chatml_prompt_file}") {
            let path = match &chatml_prompt_path {
                Some(path) => path.clone(),
                None => {
                    let prompt = chatml_prompt
                        .as_deref()
                        .unwrap_or_else(|| unreachable!("chatml prompt was prepared"));
                    let path = write_ai_prompt_file(&provider.id, prompt)?;
                    chatml_prompt_path = Some(path.clone());
                    prompt_paths.push(path.clone());
                    path
                }
            };
            value = value.replace("{chatml_prompt_file}", &path.to_string_lossy());
        }
        if value.contains("{chatml_prompt}") {
            let prompt = chatml_prompt
                .as_deref()
                .unwrap_or_else(|| unreachable!("chatml prompt was prepared"));
            value = value.replace("{chatml_prompt}", prompt);
        }
        if value.contains("{prompt}") {
            value = value.replace("{prompt}", prompt);
        }
        prepared.push(value);
    }

    Ok((prepared, prompt_paths))
}

fn call_warm_process_ai_provider(
    provider: &AiProvider,
    command: &str,
    args: &[String],
    envs: Vec<(String, String)>,
    prompt: &str,
) -> Result<String, String> {
    let timeout = Duration::from_millis(provider.timeout_ms.max(1_000));
    let key = warm_process_key(provider);
    let signature = process_signature(command, args);
    let mut cache = WARM_AI_PROCESSES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "AI process cache is unavailable".to_string())?;

    let remove_existing = if let Some(session) = cache.get_mut(&key) {
        session.signature != signature || warm_process_has_exited(session)
    } else {
        false
    };
    if remove_existing {
        cache.remove(&key);
    }

    if let Entry::Vacant(entry) = cache.entry(key.clone()) {
        let ready_marker = provider
            .ready_marker
            .as_deref()
            .unwrap_or("ready for prompts");
        let session = start_warm_ai_process(
            command,
            args,
            envs,
            signature.clone(),
            timeout,
            ready_marker,
        )?;
        entry.insert(session);
    }

    let Some(session) = cache.get_mut(&key) else {
        return Err("AI process cache did not retain the warm process".to_string());
    };

    drain_warm_process_events(session);
    let mut request = format_process_ai_prompt(provider, prompt);
    request.push_str("\\END\n");
    session
        .stdin
        .write_all(request.as_bytes())
        .and_then(|_| session.stdin.flush())
        .map_err(|error| format!("Could not write to warm AI process: {error}"))?;

    let turn_marker = provider.turn_marker.as_deref().unwrap_or("[turn ");
    let output = collect_warm_process_answer(session, timeout, turn_marker);
    if matches!(
        output,
        Err(WarmAiProcessError::Exited { .. } | WarmAiProcessError::TimedOut)
    ) {
        cache.remove(&key);
    }

    let output = output.map_err(WarmAiProcessError::into_message)?;
    let answer = clean_process_ai_answer(provider, &output.stdout);
    if !answer.is_empty() {
        return Ok(answer);
    }

    let detail = response_error_excerpt(if output.stderr.trim().is_empty() {
        &output.stdout
    } else {
        &output.stderr
    });
    Err(format!("AI process returned no answer: {detail}"))
}

fn write_ai_prompt_file(provider_id: &str, prompt: &str) -> Result<PathBuf, String> {
    let safe_id = provider_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let path = std::env::temp_dir().join(format!(
        "veyra-ai-{safe_id}-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    fs::write(&path, prompt).map_err(|error| {
        format!(
            "Could not write AI process prompt file {}: {error}",
            path.display()
        )
    })?;
    Ok(path)
}

struct AiProcessOutput {
    status: process::ExitStatus,
    stdout: String,
    stderr: String,
}

struct WarmAiProcess {
    signature: String,
    child: process::Child,
    stdin: process::ChildStdin,
    events: mpsc::Receiver<WarmAiProcessEvent>,
}

impl Drop for WarmAiProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

enum WarmAiProcessEvent {
    Stdout(String),
    Stderr(String),
}

struct WarmAiProcessTurn {
    stdout: String,
    stderr: String,
}

enum WarmAiProcessError {
    TimedOut,
    Exited { detail: String },
    Disconnected,
    WaitFailed(String),
}

impl WarmAiProcessError {
    fn into_message(self) -> String {
        match self {
            WarmAiProcessError::TimedOut => "AI process timed out".to_string(),
            WarmAiProcessError::Exited { detail } => format!("AI process exited: {detail}"),
            WarmAiProcessError::Disconnected => "AI process output stream closed".to_string(),
            WarmAiProcessError::WaitFailed(error) => {
                format!("Could not wait for AI process: {error}")
            }
        }
    }
}

static WARM_AI_PROCESSES: OnceLock<Mutex<HashMap<String, WarmAiProcess>>> = OnceLock::new();

pub(crate) fn shutdown_warm_ai_processes() {
    if let Some(cache) = WARM_AI_PROCESSES.get()
        && let Ok(mut cache) = cache.lock()
    {
        cache.clear();
    }
}

fn warm_process_key(provider: &AiProvider) -> String {
    non_empty(&provider.id)
        .or_else(|| non_empty(&provider.label))
        .unwrap_or_else(|| provider.command.clone())
}

fn process_signature(command: &str, args: &[String]) -> String {
    let mut signature = command.to_string();
    for arg in args {
        signature.push('\0');
        signature.push_str(arg);
    }
    signature
}

fn warm_process_has_exited(session: &mut WarmAiProcess) -> bool {
    matches!(session.child.try_wait(), Ok(Some(_)) | Err(_))
}

fn start_warm_ai_process(
    command: &str,
    args: &[String],
    envs: Vec<(String, String)>,
    signature: String,
    timeout: Duration,
    ready_marker: &str,
) -> Result<WarmAiProcess, String> {
    let mut child_command = process::Command::new(command);
    child_command
        .args(args)
        .envs(envs)
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped());
    #[cfg(windows)]
    child_command.creation_flags(WINDOWS_CREATE_NO_WINDOW);

    let mut child = child_command
        .spawn()
        .map_err(|error| format!("Could not start warm AI process {command}: {error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Could not open warm AI process stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Could not capture warm AI process stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Could not capture warm AI process stderr".to_string())?;
    let (sender, events) = mpsc::channel();
    read_warm_process_stdout(stdout, sender.clone());
    read_warm_process_stderr(stderr, sender);

    let mut session = WarmAiProcess {
        signature,
        child,
        stdin,
        events,
    };
    wait_for_warm_process_ready(&mut session, timeout, ready_marker)?;
    Ok(session)
}

fn wait_for_warm_process_ready(
    session: &mut WarmAiProcess,
    timeout: Duration,
    ready_marker: &str,
) -> Result<(), String> {
    let started = Instant::now();
    let mut diagnostics = String::new();
    loop {
        if let Some(status) = session
            .child
            .try_wait()
            .map_err(|error| format!("Could not wait for warm AI process: {error}"))?
        {
            return Err(format!(
                "Warm AI process exited before it was ready ({status}): {}",
                response_error_excerpt(&diagnostics)
            ));
        }

        if started.elapsed() >= timeout {
            return Err(format!(
                "Warm AI process did not become ready after {} ms",
                timeout.as_millis()
            ));
        }

        match session.events.recv_timeout(Duration::from_millis(25)) {
            Ok(WarmAiProcessEvent::Stderr(line)) => {
                if line.contains(ready_marker) {
                    return Ok(());
                }
                diagnostics.push_str(&line);
            }
            Ok(WarmAiProcessEvent::Stdout(chunk)) => {
                diagnostics.push_str(&chunk);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Warm AI process output stream closed during startup".to_string());
            }
        }
    }
}

fn drain_warm_process_events(session: &mut WarmAiProcess) {
    while session.events.try_recv().is_ok() {}
}

fn collect_warm_process_answer(
    session: &mut WarmAiProcess,
    timeout: Duration,
    turn_marker: &str,
) -> Result<WarmAiProcessTurn, WarmAiProcessError> {
    let started = Instant::now();
    let mut stdout = String::new();
    let mut stderr = String::new();

    loop {
        if let Some(status) = session
            .child
            .try_wait()
            .map_err(|error| WarmAiProcessError::WaitFailed(error.to_string()))?
        {
            return Err(WarmAiProcessError::Exited {
                detail: format!("{status}: {}", response_error_excerpt(&stderr)),
            });
        }

        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Err(WarmAiProcessError::TimedOut);
        }
        let remaining = timeout.saturating_sub(elapsed);
        let wait_for = remaining.min(Duration::from_millis(50));

        match session.events.recv_timeout(wait_for) {
            Ok(WarmAiProcessEvent::Stdout(chunk)) => stdout.push_str(&chunk),
            Ok(WarmAiProcessEvent::Stderr(line)) => {
                if line.contains(turn_marker) {
                    drain_warm_process_completion_events(session, &mut stdout, &mut stderr);
                    return Ok(WarmAiProcessTurn { stdout, stderr });
                }
                stderr.push_str(&line);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(WarmAiProcessError::Disconnected);
            }
        }
    }
}

fn drain_warm_process_completion_events(
    session: &mut WarmAiProcess,
    stdout: &mut String,
    stderr: &mut String,
) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(25) {
        match session.events.recv_timeout(Duration::from_millis(5)) {
            Ok(WarmAiProcessEvent::Stdout(chunk)) => stdout.push_str(&chunk),
            Ok(WarmAiProcessEvent::Stderr(line)) => stderr.push_str(&line),
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn read_warm_process_stdout(
    mut stdout: process::ChildStdout,
    sender: mpsc::Sender<WarmAiProcessEvent>,
) {
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let _ = sender.send(WarmAiProcessEvent::Stdout(
                        String::from_utf8_lossy(&buffer[..count]).to_string(),
                    ));
                }
                Err(_) => break,
            }
        }
    });
}

fn read_warm_process_stderr(
    stderr: process::ChildStderr,
    sender: mpsc::Sender<WarmAiProcessEvent>,
) {
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(mut line) = line else {
                break;
            };
            line.push('\n');
            let _ = sender.send(WarmAiProcessEvent::Stderr(line));
        }
    });
}

fn run_ai_process(
    command: &str,
    args: &[String],
    envs: Vec<(String, String)>,
    timeout: Duration,
) -> Result<AiProcessOutput, String> {
    let mut child_command = process::Command::new(command);
    child_command
        .args(args)
        .envs(envs)
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped());
    #[cfg(windows)]
    child_command.creation_flags(WINDOWS_CREATE_NO_WINDOW);

    let mut child = child_command
        .spawn()
        .map_err(|error| format!("Could not start AI process {command}: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Could not capture AI process stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Could not capture AI process stderr".to_string())?;
    let stdout_reader = read_process_pipe(stdout);
    let stderr_reader = read_process_pipe(stderr);
    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "AI process timed out after {} ms",
                        timeout.as_millis()
                    ));
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Could not wait for AI process: {error}"));
            }
        }
    };

    Ok(AiProcessOutput {
        status,
        stdout: stdout_reader.join().unwrap_or_default(),
        stderr: stderr_reader.join().unwrap_or_default(),
    })
}

fn read_process_pipe<R: Read + Send + 'static>(mut pipe: R) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = pipe.read_to_end(&mut buffer);
        String::from_utf8_lossy(&buffer).to_string()
    })
}

pub(crate) fn format_process_ai_prompt(provider: &AiProvider, prompt: &str) -> String {
    let prompt = prompt.trim();
    if let Some(template) = provider.prompt_template.as_deref() {
        if template.contains("{system}") && template.contains("{user}") {
            return template
                .replace("{system}", AI_SYSTEM_PROMPT)
                .replace("{user}", prompt);
        }
        if template.contains("{prompt}") {
            return template.replace("{prompt}", prompt);
        }
        if template.contains("{user}") {
            return template.replace("{user}", prompt);
        }
        return template.to_string();
    }

    format!(
        "<s><|im_start|>system\n{AI_SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",
    )
}

pub(crate) fn clean_process_ai_answer(provider: &AiProvider, raw: &str) -> String {
    let mut answer = raw.replace("\r\n", "\n");
    let stop_tokens: Vec<&str> = if provider.stop_tokens.is_empty() {
        vec!["<|im_end|>", "<|endoftext|>", "</s>"]
    } else {
        provider.stop_tokens.iter().map(String::as_str).collect()
    };
    for stop in stop_tokens {
        if let Some(index) = answer.find(stop) {
            answer.truncate(index);
        }
    }
    let trimmed = answer.trim_start();
    if trimmed.starts_with("<think>")
        && let Some(end_index) = trimmed.find("</think>")
    {
        return trimmed[end_index + "</think>".len()..].trim().to_string();
    }
    trimmed.trim_end().to_string()
}

pub(crate) fn chat_completions_url(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("AI provider base URL is empty".to_string());
    }

    let normalized = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let lowered = normalized.to_ascii_lowercase();
    if lowered.ends_with("/chat/completions") {
        Ok(normalized)
    } else if lowered.ends_with("/v1") {
        Ok(format!("{normalized}/chat/completions"))
    } else {
        Ok(format!("{normalized}/v1/chat/completions"))
    }
}

pub(crate) fn is_local_http_endpoint(endpoint: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(endpoint) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();

    host == "localhost" || host == "::1" || host == "0.0.0.0" || host.starts_with("127.")
}

pub(crate) fn parse_chat_completion_answer(raw: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("AI returned invalid JSON: {error}"))?;
    let answer = value
        .pointer("/choices/0/message/content")
        .and_then(chat_content_text)
        .or_else(|| value.pointer("/choices/0/text").and_then(chat_content_text))
        .or_else(|| {
            value
                .pointer("/message/content")
                .and_then(chat_content_text)
        })
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());

    answer.ok_or_else(|| "AI response did not include an answer".to_string())
}

fn chat_content_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(chat_content_text)
                .collect::<Vec<_>>()
                .join("");
            (!text.trim().is_empty()).then_some(text)
        }
        serde_json::Value::Object(_) => value
            .get("text")
            .or_else(|| value.get("content"))
            .and_then(chat_content_text),
        _ => None,
    }
}

pub(crate) fn response_error_excerpt(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body)
        && let Some(message) = value
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .and_then(non_empty)
    {
        return message;
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "empty response body".to_string();
    }

    let mut chars = trimmed.chars();
    let mut excerpt = chars.by_ref().take(500).collect::<String>();
    if chars.next().is_some() {
        excerpt.push_str("...");
    }
    excerpt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_process_provider() -> AiProvider {
        AiProvider {
            id: "test_process".to_string(),
            label: "Test Process".to_string(),
            kind: AiProviderKind::Process,
            base_url: String::new(),
            model: String::new(),
            command: "test.exe".to_string(),
            args: Vec::new(),
            keep_warm: false,
            api_key_env: None,
            local_only: true,
            enabled: true,
            timeout_ms: 1_000,
            supports_streaming: false,
            supports_tools: false,
            context_limit_tokens: None,
            ..Default::default()
        }
    }

    #[test]
    fn one_shot_process_args_default_to_chatml_prompt_file() {
        let provider = test_process_provider();
        let (args, paths) =
            prepare_one_shot_process_args(&provider, vec!["--temperature".into()], "hello")
                .unwrap();

        assert_eq!(args[0], "--temperature");
        assert_eq!(args[1], "--prompt-file");
        assert_eq!(paths.len(), 1);
        let prompt = fs::read_to_string(&paths[0]).unwrap();
        assert!(prompt.contains("<|im_start|>user\nhello<|im_end|>"));

        for path in paths {
            fs::remove_file(path).ok();
        }
    }

    #[test]
    fn http_ai_provider_transport_round_trip() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = mpsc::channel();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());

            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut content_length = 0_usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line == "\n" {
                    break;
                }
                if let Some(value) = line
                    .strip_prefix("Content-Length:")
                    .or_else(|| line.strip_prefix("content-length:"))
                {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0_u8; content_length];
            reader.read_exact(&mut body).unwrap();
            let _ = request_tx.send((request_line, String::from_utf8_lossy(&body).to_string()));

            let response_body = r#"{
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "model": "test-model",
                "choices": [
                    {
                        "index": 0,
                        "message": { "role": "assistant", "content": "hello from the test server" },
                        "finish_reason": "stop"
                    }
                ]
            }"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let provider = AiProvider {
            id: "http_test".to_string(),
            label: "HTTP Test".to_string(),
            kind: AiProviderKind::OpenAiCompatible,
            base_url: format!("http://{address}"),
            model: "test-model".to_string(),
            local_only: true,
            timeout_ms: 10_000,
            ..Default::default()
        };

        let answer = call_ai_provider(provider, "ping".to_string(), true).unwrap();
        assert_eq!(answer, "hello from the test server");

        let (request_line, request_body) =
            request_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(request_line.starts_with("POST /v1/chat/completions"));
        let request_json: serde_json::Value = serde_json::from_str(&request_body).unwrap();
        assert_eq!(request_json["model"], "test-model");
        assert_eq!(request_json["messages"][1]["content"], "ping");

        server.join().unwrap();
    }

    #[test]
    fn one_shot_process_args_replace_prompt_placeholders_without_default_append() {
        let provider = test_process_provider();
        let (args, paths) = prepare_one_shot_process_args(
            &provider,
            vec![
                "--print".into(),
                "{prompt}".into(),
                "--prompt-path".into(),
                "{prompt_file}".into(),
                "--chatml-path".into(),
                "{chatml_prompt_file}".into(),
            ],
            "raw prompt",
        )
        .unwrap();

        assert_eq!(args[0], "--print");
        assert_eq!(args[1], "raw prompt");
        assert!(!args.contains(&"--prompt-file".to_string()));
        assert_eq!(paths.len(), 2);
        assert_eq!(fs::read_to_string(&paths[0]).unwrap(), "raw prompt");
        assert!(
            fs::read_to_string(&paths[1])
                .unwrap()
                .contains("<|im_start|>user\nraw prompt<|im_end|>")
        );

        for path in paths {
            fs::remove_file(path).ok();
        }
    }
}
