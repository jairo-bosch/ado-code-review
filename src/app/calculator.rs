use crate::Error;

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl Operation {
    pub(super) fn calculate(self, left: f64, right: f64) -> Result<f64, Error> {
        if !left.is_finite() || !right.is_finite() {
            return Err(Error::NonFiniteOperand);
        }
        let value = match self {
            Self::Add => left + right,
            Self::Subtract => left - right,
            Self::Multiply => left * right,
            Self::Divide => divide(left, right)?,
        };
        if value.is_finite() {
            Ok(value)
        } else {
            Err(Error::NonFiniteResult)
        }
    }
}

fn divide(left: f64, right: f64) -> Result<f64, Error> {
    if right == 0.0 {
        Err(Error::DivisionByZero)
    } else {
        Ok(left / right)
    }
}

#[cfg(test)]
mod tests;
