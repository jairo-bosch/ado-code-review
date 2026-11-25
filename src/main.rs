use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    match ado_code_review::run(env::args_os(), &mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => match writeln!(io::stderr().lock(), "{error}") {
            Ok(()) => error.exit_code(),
            Err(_) => ExitCode::FAILURE,
        },
    }
}
