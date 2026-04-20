// build.rs — bake .env values into the binary at compile time so Android doesn't
// need runtime file access.  Values can still be overridden by setting the env
// vars directly in the shell before running `cargo build`.

fn main() {
    // Load .env if present (ignore missing file — CI may set vars directly)
    if let Ok(contents) = std::fs::read_to_string(".env") {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"');
                // Only emit vars that aren't already set in the environment
                if std::env::var(key).is_err() {
                    println!("cargo:rustc-env={}={}", key, value);
                }
            }
        }
    }

    // Re-run if .env changes
    println!("cargo:rerun-if-changed=.env");
    println!("cargo:rerun-if-env-changed=POLLICORE_URL");
}
