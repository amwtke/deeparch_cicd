use crate::ci::detector::ProjectInfo;

/// Lockfile + optional `.nvmrc` vs container Node major (first line of shell pipeline).
pub fn vue_hygiene_shell() -> String {
    r#"if [ ! -f package-lock.json ]; then echo "ERROR: package-lock.json missing — run npm install and commit the lockfile"; exit 1; fi && if [ -f .nvmrc ]; then WANT=$(grep -v '^#' .nvmrc | head -1 | tr -d 'v \r'); MAJOR_WANT=${WANT%%.*}; GOT=$(node -p "process.version.slice(1).split('.')[0]"); if [ "$MAJOR_WANT" != "$GOT" ]; then echo "ERROR: Node major mismatch: .nvmrc expects major $MAJOR_WANT, container has $GOT (image must match .nvmrc)"; exit 1; fi; fi && echo "node hygiene: package-lock.json present; .nvmrc matches container (or no .nvmrc)""#.to_string()
}

/// Run hygiene before the rest of the step (same container, fewer scheduled steps).
pub fn prepend_hygiene(commands: Vec<String>) -> Vec<String> {
    let mut v = vec![vue_hygiene_shell()];
    v.extend(commands);
    v
}

pub fn hygiene_context_paths(info: &ProjectInfo) -> Vec<String> {
    let mut paths = vec![
        "package.json".to_string(),
        "package-lock.json".to_string(),
        ".nvmrc".to_string(),
    ];
    paths.extend(info.config_files.iter().cloned());
    paths.sort();
    paths.dedup();
    paths
}

/// Hygiene script failed (distinct from lint/test/type errors).
pub fn hygiene_failure_in_output(stdout: &str, stderr: &str) -> bool {
    let o = format!("{stdout}{stderr}");
    o.contains("package-lock.json missing") || o.contains("Node major mismatch")
}
