use crate::Error;

#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl Operation {
    pub(crate) fn calculate(self, left: f64, right: f64) -> Result<f64, Error> {
        if !left.is_finite() || !right.is_finite() {
            return Err(Error::NonFiniteOperand);
        }
        let value = match self {
            Self::Add => left + right,
            Self::Subtract => left - right,
            Self::Multiply => left * right,
            Self::Divide if right == 0.0 => return Err(Error::DivisionByZero),
            Self::Divide => left / right,
        };
        if value.is_finite() {
            Ok(value)
        } else {
            Err(Error::NonFiniteResult)
        }
    }
}

#[cfg(test)]
mod tests;
