use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    let mut stderr = io::stderr();
    match ado_code_review::run_with_progress(env::args_os(), &mut io::stdout().lock(), &mut stderr)
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => match writeln!(stderr, "{error}") {
            Ok(()) => error.exit_code(),
            Err(_) => ExitCode::FAILURE,
        },
    }
}
