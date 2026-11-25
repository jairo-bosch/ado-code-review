use std::io::{self, Write};

use ado_code_review::{Error, run};

#[test]
fn writes_to_the_supplied_stream() {
    let mut output = Vec::new();
    run(["calculator", "subtract", "-2", "3"], &mut output).unwrap();
    assert_eq!(output, b"-5\n");
}

#[test]
fn reports_output_failure() {
    assert!(matches!(
        run(["calculator", "add", "1", "2"], &mut BrokenOutput),
        Err(Error::Output(error)) if error.kind() == io::ErrorKind::BrokenPipe
    ));
}

struct BrokenOutput;

impl Write for BrokenOutput {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
