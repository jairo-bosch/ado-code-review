use clap::{Args, Parser, Subcommand};

use super::{Error, calculator::Operation};

#[derive(Parser)]
#[command(version, about = "Perform arithmetic on two numbers")]
pub(super) struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add the operands.
    Add(Operands),
    /// Subtract the right operand from the left operand.
    Subtract(Operands),
    /// Multiply the operands.
    Multiply(Operands),
    /// Divide the left operand by the right operand.
    Divide(Operands),
}

#[derive(Args)]
struct Operands {
    // Signed exponents and non-finite values need hyphen support beyond clap's decimal check.
    #[arg(allow_negative_numbers = true, allow_hyphen_values = true)]
    left: f64,
    #[arg(allow_negative_numbers = true, allow_hyphen_values = true)]
    right: f64,
}

impl Cli {
    pub(super) fn calculate(self) -> Result<f64, Error> {
        let (operation, operands) = match self.command {
            Command::Add(values) => (Operation::Add, values),
            Command::Subtract(values) => (Operation::Subtract, values),
            Command::Multiply(values) => (Operation::Multiply, values),
            Command::Divide(values) => (Operation::Divide, values),
        };
        operation.calculate(operands.left, operands.right)
    }
}
