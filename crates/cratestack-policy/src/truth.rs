//! Three-valued truth for procedure policies.
//!
//! A comparison between a number and a string that is not a number
//! ([`crate::compare::Comparison::Undecidable`]) is neither true nor false.
//! Collapsing it to a `bool` is what fails open: as `false` it lets a `@deny`
//! built on `==` stay silent, as `true` it lets an `@allow` built on `!=`
//! pass. SQL's three-valued logic already refuses it on the read path; this
//! is the same logic for the procedure evaluator.
//!
//! The connectives are Kleene's. `false` decides a conjunction and `true`
//! decides a disjunction whatever stands beside them; otherwise an `Unknown`
//! operand leaves the result `Unknown`. An `@allow` grants only on `True`; an
//! `@deny` fires on anything that is not `False`.
//!
//! No predicate other than the claim comparisons produces `Unknown`, so a
//! policy that does not compare a claim with a number evaluates exactly as it
//! did when predicates were plain `bool`s.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Truth {
    True,
    False,
    Unknown,
}

impl From<bool> for Truth {
    fn from(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }
}

impl Truth {
    /// `&&` over any number of operands; an empty conjunction is `True`.
    pub(crate) fn all(operands: impl IntoIterator<Item = Self>) -> Self {
        let mut result = Self::True;
        for operand in operands {
            match operand {
                Self::False => return Self::False,
                Self::Unknown => result = Self::Unknown,
                Self::True => {}
            }
        }
        result
    }

    /// `||` over any number of operands; an empty disjunction is `False`.
    pub(crate) fn any(operands: impl IntoIterator<Item = Self>) -> Self {
        let mut result = Self::False;
        for operand in operands {
            match operand {
                Self::True => return Self::True,
                Self::Unknown => result = Self::Unknown,
                Self::False => {}
            }
        }
        result
    }

    /// What an `@allow` needs.
    pub(crate) fn is_true(self) -> bool {
        self == Self::True
    }

    /// What lets a `@deny` stay silent.
    pub(crate) fn is_false(self) -> bool {
        self == Self::False
    }
}

#[cfg(test)]
mod tests {
    use super::Truth::{self, False, True, Unknown};

    #[test]
    fn conjunction_is_decided_by_false_and_disjunction_by_true() {
        assert_eq!(Truth::all([True, Unknown, False]), False);
        assert_eq!(Truth::all([Unknown, True]), Unknown);
        assert_eq!(Truth::all([True, True]), True);
        assert_eq!(Truth::all([]), True);
        assert_eq!(Truth::any([False, Unknown, True]), True);
        assert_eq!(Truth::any([Unknown, False]), Unknown);
        assert_eq!(Truth::any([False, False]), False);
        assert_eq!(Truth::any([]), False);
    }

    #[test]
    fn unknown_is_neither_true_nor_false() {
        assert!(!Unknown.is_true());
        assert!(!Unknown.is_false());
        assert!(True.is_true() && !True.is_false());
        assert!(False.is_false() && !False.is_true());
        assert_eq!(Truth::from(true), True);
        assert_eq!(Truth::from(false), False);
    }
}
