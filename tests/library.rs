use std::io::{self, Write};

use ado_code_review::{Error, run};

#[test]
fn writes_to_the_supplied_stream() {
    let mut output = Vec::new();
    run(["ado-code-review", "--help"], &mut output).unwrap();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("Review Azure DevOps pull requests")
    );
}

#[test]
fn reports_output_failure() {
    assert!(matches!(
        run(["ado-code-review", "--help"], &mut BrokenOutput),
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
