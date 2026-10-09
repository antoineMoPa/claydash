//! Native launch routing. Validate arguments before creating a window or GPU.
#[derive(Debug, PartialEq, Eq)]
pub enum Launch {
    Desktop,
    Help,
    #[cfg(unix)]
    Agent,
}

pub fn parse(args: &[String]) -> Result<Launch, String> {
    let Some(first) = args.first() else {
        return Ok(Launch::Desktop);
    };
    match first.as_str() {
        "--help" | "-h" | "help" => {
            if let Some(extra) = args.get(1) {
                return Err(format!("unexpected argument: {extra}"));
            }
            return Ok(Launch::Help);
        }
        #[cfg(unix)]
        "serve" | "--headless" | "--agent-headless" | "mcp" | "agent" => return Ok(Launch::Agent),
        _ => {}
    }
    // These existing developer workflows read their values at the point of use.
    // Match exact option names so a typo cannot silently launch the desktop app.
    for arg in args {
        if matches!(
            arg.as_str(),
            "--stress-benchmark"
                | "--stress-benchmark-suite"
                | "--stress-benchmark-brute-force"
                | "--stress-ui-benchmark"
                | "--ui-preview"
                | "--benchmark-outline"
                | "--benchmark-orthographic"
                | "--benchmark-edit"
                | "--benchmark-move-group"
                | "--benchmark-animation"
                | "--benchmark-selection-click"
                | "--benchmark-group-workflow"
                | "--benchmark-default-group-transitions"
                | "--benchmark-deferred"
                | "--benchmark-fast-preview"
                | "--benchmark-progressive"
                | "--benchmark-1024"
        ) {
            continue;
        }
        if let Some((name, value)) = arg.split_once('=') {
            if matches!(
                name,
                "--benchmark-scene"
                    | "--benchmark-case"
                    | "--benchmark-size"
                    | "--benchmark-shader"
                    | "--benchmark-images"
                    | "--guide-panel"
                    | "--guide-theme"
                    | "--guide-screenshot"
            ) {
                if value.is_empty() {
                    return Err(format!("{name} requires a value"));
                }
                continue;
            }
        }
        return Err(format!("unknown command or option: {arg}"));
    }
    Ok(Launch::Desktop)
}

pub fn print_help() {
    println!("Claydash — 3D modeling and local automation\n\nUsage: claydash [COMMAND] [OPTIONS]\n\nRun without arguments to open the desktop app.\n\nOptions:\n  -h, --help        Print this help");
    #[cfg(unix)]
    println!("\nCommands:\n  serve            Run a server with no app window\n  mcp              Connect an MCP stdio client to an existing instance\n  mcp --headless   Run a private, window-free MCP session\n  agent OP [JSON]  Send an operation to an existing instance\n\nServer aliases:\n  --headless, --agent-headless\n\nHeadless options:\n  --scene PATH         Load a .claydash document\n  --size WIDTHxHEIGHT  Capture size (default: 640x480)\n\nEnvironment:\n  CLAYDASH_AGENT_SOCKET  Override the local server/client socket path\n\nExamples:\n  claydash --headless\n  claydash serve --scene duck.claydash --size 1024x768\n  claydash mcp --headless\n  claydash agent GetState\n\nUse claydash agent --help for operation examples.");
    println!("\nDeveloper workflows:\n  --stress-benchmark, --stress-benchmark-suite, --stress-ui-benchmark\n  --ui-preview --guide-screenshot=PATH");
}

#[cfg(test)]
mod tests {
    use super::*;
    fn launch(args: &[&str]) -> Result<Launch, String> {
        parse(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn unknown_arguments_never_route_to_the_desktop() {
        for args in [
            &["nonsense"][..],
            &["--healp"],
            &["--ui-preview", "--typo"],
            &["--stress-benchmark", "unexpected"],
            &["--benchmark-size="],
            &["--help", "extra"],
        ] {
            assert!(launch(args).is_err(), "{args:?}");
        }
        assert_eq!(launch(&[]), Ok(Launch::Desktop));
        assert_eq!(
            launch(&["--stress-benchmark", "--benchmark-size=128x128"]),
            Ok(Launch::Desktop)
        );
        assert_eq!(
            launch(&["--ui-preview", "--guide-screenshot=/tmp/guide.png"]),
            Ok(Launch::Desktop)
        );
    }
    #[test]
    fn help_and_headless_have_explicit_routes() {
        for arg in ["--help", "-h", "help"] {
            assert_eq!(launch(&[arg]), Ok(Launch::Help));
        }
        #[cfg(unix)]
        for arg in ["serve", "--headless", "--agent-headless", "mcp", "agent"] {
            assert_eq!(launch(&[arg]), Ok(Launch::Agent));
        }
    }
}
