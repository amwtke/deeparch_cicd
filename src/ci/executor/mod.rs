use anyhow::{Context, Result};
use bollard::container::{
    Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions,
    WaitContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::Docker;
use futures_util::StreamExt;
use tracing::{debug, info, warn};

use crate::ci::parser::Step;

/// Result of executing a single step
#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_name: String,
    pub exit_code: i64,
    pub logs: Vec<LogLine>,
    pub duration: std::time::Duration,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub stream: LogStream,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl StepResult {
    pub fn stdout_string(&self) -> String {
        self.logs
            .iter()
            .filter(|l| l.stream == LogStream::Stdout)
            .map(|l| l.message.as_str())
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn stderr_string(&self) -> String {
        self.logs
            .iter()
            .filter(|l| l.stream == LogStream::Stderr)
            .map(|l| l.message.as_str())
            .collect::<Vec<_>>()
            .join("")
    }
}

/// Executes pipeline steps in Docker containers
#[derive(Clone)]
pub struct DockerExecutor {
    docker: Docker,
}

impl DockerExecutor {
    pub async fn new() -> Result<Self> {
        let docker = Self::connect().await?;

        // Verify connection
        docker
            .ping()
            .await
            .context("Cannot reach Docker daemon. Is Docker running?")?;

        Ok(Self { docker })
    }

    /// Try to connect to Docker daemon, probing multiple socket locations.
    /// Priority: $DOCKER_HOST env var → platform-specific paths → bollard defaults.
    async fn connect() -> Result<Docker> {
        // If DOCKER_HOST is set, let bollard handle it
        if std::env::var("DOCKER_HOST").is_ok() {
            return Docker::connect_with_local_defaults()
                .context("Failed to connect to Docker daemon via $DOCKER_HOST");
        }

        // Probe known socket paths
        let candidates = Self::socket_candidates();
        for path in &candidates {
            if std::path::Path::new(path).exists() {
                return Docker::connect_with_unix(path, 120, bollard::API_DEFAULT_VERSION)
                    .context(format!("Failed to connect to Docker daemon at {}", path));
            }
        }

        // Fallback to bollard defaults
        Docker::connect_with_local_defaults().context(format!(
            "Failed to connect to Docker daemon: Socket not found. Tried: {}",
            candidates.join(", ")
        ))
    }

    /// Return candidate Docker socket paths for the current platform.
    fn socket_candidates() -> Vec<String> {
        let mut paths = vec![];

        // macOS: Docker Desktop puts socket in ~/.docker/run/
        if let Some(home) = dirs::home_dir() {
            paths.push(format!("{}/.docker/run/docker.sock", home.display()));
        }

        // Linux standard path / macOS fallback
        paths.push("/var/run/docker.sock".to_string());

        // Colima (macOS alternative)
        if let Some(home) = dirs::home_dir() {
            paths.push(format!("{}/.colima/default/docker.sock", home.display()));
        }

        paths
    }

    /// Execute a single pipeline step inside a Docker container.
    /// `project_dir` is bind-mounted into the container at the step's workdir.
    pub async fn run_step(
        &self,
        pipeline_name: &str,
        step: &Step,
        project_dir: &std::path::Path,
        on_log: impl Fn(&LogLine) + Send,
    ) -> Result<StepResult> {
        let start = std::time::Instant::now();
        let container_name = format!(
            "pipelight-{}-{}-{}",
            pipeline_name,
            step.name,
            uuid::Uuid::new_v4().to_string()[..8].to_string()
        );

        info!(step = %step.name, image = %step.image, "Starting step");

        // Pull image if needed
        self.ensure_image(&step.image).await?;

        // Build the shell script from commands
        let script = step.commands.join(" && ");
        let entrypoint = vec!["/bin/sh".to_string(), "-c".to_string()];
        let cmd = vec![script];

        // Build environment variables
        let env: Vec<String> = step
            .env
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Bind mount project directory into container workdir
        let host_path = project_dir
            .canonicalize()
            .context("Failed to resolve project directory")?;
        let mut binds = vec![format!("{}:{}", host_path.display(), step.workdir)];

        // Add extra volume mounts from step config (e.g., cache directories)
        for vol in &step.volumes {
            // Expand ~ to home directory
            let expanded = if vol.starts_with("~/") || vol.starts_with("~:") {
                if let Some(home) = dirs::home_dir() {
                    vol.replacen('~', &home.display().to_string(), 1)
                } else {
                    vol.clone()
                }
            } else {
                vol.clone()
            };
            // Only mount if host path exists
            let host_part = expanded.split(':').next().unwrap_or("");
            if std::path::Path::new(host_part).exists() {
                binds.push(expanded);
            }
        }

        let host_config = HostConfig {
            binds: Some(binds),
            network_mode: Some("host".to_string()),
            ..Default::default()
        };

        // Run container as current user to avoid root-owned files on bind mounts
        let uid = unsafe { libc::getuid() };
        let gid = unsafe { libc::getgid() };
        let user_str = format!("{}:{}", uid, gid);

        // Create container
        let config = Config {
            image: Some(step.image.clone()),
            entrypoint: Some(entrypoint),
            cmd: Some(cmd),
            working_dir: Some(step.workdir.clone()),
            env: Some(env),
            user: Some(user_str),
            host_config: Some(host_config),
            ..Default::default()
        };

        let container = self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.clone(),
                    ..Default::default()
                }),
                config,
            )
            .await
            .context(format!(
                "Failed to create container for step '{}'",
                step.name
            ))?;

        debug!(container_id = %container.id, "Container created");

        // Start container
        self.docker
            .start_container(&container.id, None::<StartContainerOptions<String>>)
            .await
            .context("Failed to start container")?;

        // Collect logs
        let mut logs = Vec::new();
        let mut log_stream = self.docker.logs::<String>(
            &container.id,
            Some(LogsOptions {
                follow: true,
                stdout: true,
                stderr: true,
                ..Default::default()
            }),
        );

        while let Some(result) = log_stream.next().await {
            match result {
                Ok(output) => {
                    let (stream, message) = match output {
                        bollard::container::LogOutput::StdOut { message } => (
                            LogStream::Stdout,
                            String::from_utf8_lossy(&message).to_string(),
                        ),
                        bollard::container::LogOutput::StdErr { message } => (
                            LogStream::Stderr,
                            String::from_utf8_lossy(&message).to_string(),
                        ),
                        _ => continue,
                    };
                    let line = LogLine { stream, message };
                    on_log(&line);
                    logs.push(line);
                }
                Err(e) => {
                    warn!(error = %e, "Error reading container logs");
                    break;
                }
            }
        }

        // Wait for container to finish
        let mut wait_stream = self.docker.wait_container(
            &container.id,
            Some(WaitContainerOptions {
                condition: "not-running",
            }),
        );

        let exit_code = if let Some(result) = wait_stream.next().await {
            match result {
                Ok(response) => response.status_code,
                Err(e) => {
                    warn!(error = %e, "Error waiting for container");
                    -1
                }
            }
        } else {
            -1
        };

        // Cleanup container
        let _ = self
            .docker
            .remove_container(
                &container.id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        let duration = start.elapsed();
        let success = exit_code == 0 || step.allow_failure;

        Ok(StepResult {
            step_name: step.name.clone(),
            exit_code,
            logs,
            duration,
            success,
        })
    }

    /// Execute a single pipeline step locally (no Docker container).
    pub async fn run_step_local(
        step: &Step,
        project_dir: &std::path::Path,
        on_log: impl Fn(&LogLine) + Send,
    ) -> Result<StepResult> {
        let start = std::time::Instant::now();

        info!(step = %step.name, "Starting local step");

        let script = step.commands.join(" && ");

        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .current_dir(project_dir)
            .envs(step.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .context(format!("Failed to execute local step '{}'", step.name))?;

        let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();

        let mut logs = Vec::new();
        if !stdout_str.is_empty() {
            let line = LogLine {
                stream: LogStream::Stdout,
                message: stdout_str,
            };
            on_log(&line);
            logs.push(line);
        }
        if !stderr_str.is_empty() {
            let line = LogLine {
                stream: LogStream::Stderr,
                message: stderr_str,
            };
            on_log(&line);
            logs.push(line);
        }

        let exit_code = output.status.code().unwrap_or(-1) as i64;
        let duration = start.elapsed();
        let success = exit_code == 0 || step.allow_failure;

        Ok(StepResult {
            step_name: step.name.clone(),
            exit_code,
            logs,
            duration,
            success,
        })
    }

    /// Pull image if not available locally
    /// Pull an image if not already available locally.
    pub async fn pull_image(&self, image: &str) -> Result<()> {
        self.ensure_image(image).await
    }

    async fn ensure_image(&self, image: &str) -> Result<()> {
        // Check if image exists locally
        if self.docker.inspect_image(image).await.is_ok() {
            debug!(image = %image, "Image already available");
            return Ok(());
        }

        info!(image = %image, "Pulling image...");
        let mut stream = self.docker.create_image(
            Some(CreateImageOptions {
                from_image: image,
                ..Default::default()
            }),
            None,
            None,
        );

        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(status) = info.status {
                        debug!(status = %status, "Pull progress");
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to pull image '{}': {}", image, e));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdout_string() {
        let result = StepResult {
            step_name: "test".into(),
            exit_code: 0,
            logs: vec![
                LogLine {
                    stream: LogStream::Stdout,
                    message: "line1\n".into(),
                },
                LogLine {
                    stream: LogStream::Stderr,
                    message: "err\n".into(),
                },
                LogLine {
                    stream: LogStream::Stdout,
                    message: "line2\n".into(),
                },
            ],
            duration: std::time::Duration::from_secs(1),
            success: true,
        };
        assert_eq!(result.stdout_string(), "line1\nline2\n");
    }

    #[test]
    fn test_stderr_string() {
        let result = StepResult {
            step_name: "test".into(),
            exit_code: 1,
            logs: vec![
                LogLine {
                    stream: LogStream::Stdout,
                    message: "ok\n".into(),
                },
                LogLine {
                    stream: LogStream::Stderr,
                    message: "error1\n".into(),
                },
                LogLine {
                    stream: LogStream::Stderr,
                    message: "error2\n".into(),
                },
            ],
            duration: std::time::Duration::from_secs(1),
            success: false,
        };
        assert_eq!(result.stderr_string(), "error1\nerror2\n");
    }

    #[test]
    fn test_empty_logs() {
        let result = StepResult {
            step_name: "test".into(),
            exit_code: 0,
            logs: vec![],
            duration: std::time::Duration::from_secs(0),
            success: true,
        };
        assert_eq!(result.stdout_string(), "");
        assert_eq!(result.stderr_string(), "");
    }
}
