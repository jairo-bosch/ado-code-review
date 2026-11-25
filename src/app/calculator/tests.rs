use proptest::prelude::*;
use rstest::rstest;

use super::Operation;
use crate::Error;

#[rstest]
#[case(Operation::Add, (2.0, 3.0), 5.0)]
#[case(Operation::Subtract, (2.0, 3.0), -1.0)]
#[case(Operation::Multiply, (-2.0, -3.0), 6.0)]
#[case(Operation::Divide, (7.0, 2.0), 3.5)]
fn evaluates(#[case] op: Operation, #[case] operands: (f64, f64), #[case] expected: f64) {
    let (a, b) = operands;
    assert!((op.calculate(a, b).unwrap() - expected).abs() < f64::EPSILON);
}

#[test]
fn rejects_invalid_arithmetic() {
    assert!(matches!(
        Operation::Divide.calculate(1.0, -0.0),
        Err(Error::DivisionByZero)
    ));
    assert!(matches!(
        Operation::Add.calculate(f64::NAN, 1.0),
        Err(Error::NonFiniteOperand)
    ));
    assert!(matches!(
        Operation::Add.calculate(1.0, f64::INFINITY),
        Err(Error::NonFiniteOperand)
    ));
    assert!(matches!(
        Operation::Multiply.calculate(f64::MAX, 2.0),
        Err(Error::NonFiniteResult)
    ));
}

proptest! {
    #[test]
    fn addition_and_subtraction_round_trip(a in any::<i16>(), b in any::<i16>()) {
        let left = f64::from(a);
        let right = f64::from(b);
        let sum = Operation::Add.calculate(left, right).unwrap();
        let restored = Operation::Subtract.calculate(sum, right).unwrap();
        prop_assert!((restored - left).abs() < f64::EPSILON);
    }
}
