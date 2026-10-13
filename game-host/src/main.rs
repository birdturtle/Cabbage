fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let result = if args.len() == 3 && args[1] == "--update-launcher" {
        cabbage_game_host::update::run(std::path::Path::new(&args[2])).map(|_| 0)
    } else if args.len() == 3 {
        cabbage_game_host::host(
            std::path::Path::new(&args[1]),
            std::path::Path::new(&args[2]),
        )
    } else {
        Err("Invalid RuneScape host invocation.".into())
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
