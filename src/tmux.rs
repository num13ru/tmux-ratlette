use std::ffi::OsString;
use std::process::Command;

pub(crate) fn run(arguments: &[&str]) -> Result<String, String> {
    run_os(&arguments.iter().map(OsString::from).collect::<Vec<_>>())
}

pub(crate) fn display_current(format: &str) -> Result<String, String> {
    let mut arguments = vec![OsString::from("display-message"), OsString::from("-p")];
    if let Some(pane_id) = std::env::var_os("TMUX_PANE").filter(|value| !value.is_empty()) {
        arguments.push(OsString::from("-t"));
        arguments.push(pane_id);
    }
    arguments.push(OsString::from(format));
    run_os(&arguments)
}

pub(crate) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_os(arguments: &[OsString]) -> Result<String, String> {
    let executable = std::env::var_os("TMUX_BIN").unwrap_or_else(|| OsString::from("tmux"));
    let output = Command::new(&executable)
        .args(arguments)
        .output()
        .map_err(|error| format!("could not run {}: {error}", executable.to_string_lossy()))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!(
                "tmux {} failed with {}",
                arguments[0].to_string_lossy(),
                output.status
            )
        } else {
            format!("tmux {} failed: {detail}", arguments[0].to_string_lossy())
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_single_quotes_for_the_wrapper_shell_protocol() {
        assert_eq!(quote("a'b"), "'a'\\''b'");
    }
}
