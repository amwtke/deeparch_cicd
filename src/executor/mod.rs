use anyhow::{Context, Result};
use bollard::container::{
    Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions,
    WaitContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::Docker;
use futures_util::StreamExt;
use tracing::{debug, info, warn};

use crate::pipeline::Step;

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

/// Executes pipeline steps in Docker containers
#[derive(Clone)]
pub struct DockerExecutor {
    docker: Docker,
}

impl DockerExecutor {
    pub async fn new() -> Result<Self> {
        let docker =
            Docker::connect_with_local_defaults().context("Failed to connect to Docker daemon")?;

        // Verify connection
        docker
            .ping()
            .await
            .context("Cannot reach Docker daemon. Is Docker running?")?;

        Ok(Self { docker })
    }

    /// Execute a single pipeline step inside a Docker container
    pub async fn run_step(&self, pipeline_name: &str, step: &Step) -> Result<StepResult> {
        let start = std::time::Instant::now();
        let container_name = format!("pipelight-{}-{}-{}", pipeline_name, step.name, uuid::Uuid::new_v4().to_string()[..8].to_string());

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

        // Create container
        let config = Config {
            image: Some(step.image.clone()),
            entrypoint: Some(entrypoint),
            cmd: Some(cmd),
            working_dir: Some(step.workdir.clone()),
            env: Some(env),
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
            .context(format!("Failed to create container for step '{}'", step.name))?;

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
                        bollard::container::LogOutput::StdOut { message } => {
                            (LogStream::Stdout, String::from_utf8_lossy(&message).to_string())
                        }
                        bollard::container::LogOutput::StdErr { message } => {
                            (LogStream::Stderr, String::from_utf8_lossy(&message).to_string())
                        }
                        _ => continue,
                    };
                    logs.push(LogLine { stream, message });
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

    /// Pull image if not available locally
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
