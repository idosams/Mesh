//! Rows and the two values a column can hold.
//!
//! One row shape serves three surfaces that must agree or the reconstruction check is worthless:
//! the in-memory fold produces rows, [`crate::CommitPlan`] renders rows into SQL, and the
//! reconstruction test parses rows back out of a real database. A separate representation for each
//! is how the three drift.
//!
//! # Ordering matches SQLite's, on purpose
//!
//! [`Row`] derives `Ord`, and the derived order over `Vec<Value>` is column-by-column, `Integer`
//! numerically and `Blob` by `memcmp` — which is exactly what `ORDER BY 1, 2, …` does over a
//! `STRICT` table whose columns each hold one storage class. So sorting rows in memory and letting
//! SQLite sort them yield the same sequence, and the digest over the two can be compared directly.
//! `tests/reconstruction.rs` checks that against a real database rather than trusting the
//! paragraph you have just read.

use crate::digest::{DigestWriter, IndexDigest};

/// A value in a column: the two storage classes [`crate::ColumnType`] allows.
///
/// There is no `Null` and no `Text`. No column in [`crate::TABLES`] is nullable, and no
/// record-derived column holds free text — both are constraints the schema keeps, not accidents,
/// and both are what let [`Row::to_sql_literals`] render SQL with no quoting at all.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    /// A 64-bit signed integer.
    Integer(i64),
    /// A byte string.
    Blob(Vec<u8>),
}

impl Value {
    /// A blob from any byte source.
    #[must_use]
    pub fn blob(bytes: impl AsRef<[u8]>) -> Self {
        Self::Blob(bytes.as_ref().to_vec())
    }

    /// A non-negative count as an integer.
    ///
    /// Saturates at [`i64::MAX`] rather than wrapping. A saturated count is visibly wrong; a
    /// wrapped one reads as a small negative number that a `CHECK (… >= 0)` then rejects at a
    /// place far from the cause. Every count in this crate is a byte length or a sequence number,
    /// and neither reaches nine quintillion without something else having failed first.
    #[must_use]
    pub fn count(value: u64) -> Self {
        Self::Integer(i64::try_from(value).unwrap_or(i64::MAX))
    }

    /// The SQLite literal for this value: a bare integer, or an `X'…'` blob literal.
    ///
    /// Total, and it never quotes a string, so there is no escaping to get wrong.
    #[must_use]
    pub fn to_sql_literal(&self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::Blob(bytes) => {
                let mut text = String::with_capacity(bytes.len() * 2 + 3);
                text.push_str("X'");
                for byte in bytes {
                    text.push(hex_digit(byte >> 4));
                    text.push(hex_digit(byte & 0x0f));
                }
                text.push('\'');
                text
            }
        }
    }
}

const fn hex_digit(nibble: u8) -> char {
    (if nibble < 10 {
        b'0' + nibble
    } else {
        b'a' + nibble - 10
    }) as char
}

/// One row: its values, in the declaration order of its table's columns.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Row(Vec<Value>);

impl Row {
    /// Build a row from values in column order.
    #[must_use]
    pub const fn new(values: Vec<Value>) -> Self {
        Self(values)
    }

    /// The values, in column order.
    #[must_use]
    pub fn values(&self) -> &[Value] {
        &self.0
    }

    /// How many values the row holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the row holds no value at all, which no table produces.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The row rendered as a comma-separated SQL literal list, for a `VALUES (…)` clause.
    #[must_use]
    pub fn to_sql_literals(&self) -> String {
        self.0
            .iter()
            .map(Value::to_sql_literal)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Absorb one table's rows into a digest, framed so nothing collides.
pub(crate) fn absorb_table<D: IndexDigest>(
    writer: &mut DigestWriter<D>,
    table_name: &str,
    rows: &[Row],
) {
    writer.text(table_name);
    writer.count(rows.len());
    for row in rows {
        writer.count(row.len());
        for value in row.values() {
            match value {
                Value::Integer(number) => {
                    writer.tag(0);
                    writer.integer(*number);
                }
                Value::Blob(bytes) => {
                    writer.tag(1);
                    writer.bytes(bytes);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::Fnv1a128;

    #[test]
    fn an_integer_literal_is_bare() {
        assert_eq!(Value::Integer(42).to_sql_literal(), "42");
        assert_eq!(Value::Integer(-1).to_sql_literal(), "-1");
        assert_eq!(Value::Integer(0).to_sql_literal(), "0");
    }

    #[test]
    fn a_blob_literal_is_hex() {
        assert_eq!(Value::blob([0xde, 0xad]).to_sql_literal(), "X'dead'");
        assert_eq!(Value::blob([0x00]).to_sql_literal(), "X'00'");
        assert_eq!(Value::blob([]).to_sql_literal(), "X''");
    }

    /// The reason there is no `Text`: a blob literal has no delimiter inside it to escape, so a
    /// value carrying a quote, a backslash or a NUL cannot end the literal early.
    #[test]
    fn no_byte_can_escape_a_blob_literal() {
        for byte in 0u8..=255 {
            let literal = Value::blob([byte]).to_sql_literal();
            assert_eq!(literal.len(), 5, "unexpected literal {literal}");
            assert!(literal.starts_with("X'") && literal.ends_with('\''));
            assert_eq!(literal.matches('\'').count(), 2);
        }
    }

    #[test]
    fn a_count_saturates_rather_than_wrapping() {
        assert_eq!(Value::count(7), Value::Integer(7));
        assert_eq!(Value::count(u64::MAX), Value::Integer(i64::MAX));
    }

    /// The claim the reconstruction digest rests on, in memory. Its counterpart against a real
    /// database is `tests/reconstruction.rs`.
    #[test]
    fn row_ordering_is_column_by_column() {
        let mut rows = vec![
            Row::new(vec![Value::blob([2]), Value::Integer(1)]),
            Row::new(vec![Value::blob([1]), Value::Integer(9)]),
            Row::new(vec![Value::blob([1]), Value::Integer(2)]),
        ];
        rows.sort();
        assert_eq!(
            rows,
            vec![
                Row::new(vec![Value::blob([1]), Value::Integer(2)]),
                Row::new(vec![Value::blob([1]), Value::Integer(9)]),
                Row::new(vec![Value::blob([2]), Value::Integer(1)]),
            ]
        );
    }

    #[test]
    fn a_shorter_blob_sorts_before_its_own_extension() {
        assert!(Value::blob([1, 2]) < Value::blob([1, 2, 0]));
    }

    #[test]
    fn absorbing_two_tables_with_swapped_names_differs() {
        let rows = vec![Row::new(vec![Value::Integer(1)])];
        let mut first = DigestWriter::<Fnv1a128>::new();
        absorb_table(&mut first, "alpha", &rows);
        let mut second = DigestWriter::<Fnv1a128>::new();
        absorb_table(&mut second, "beta", &rows);
        assert_ne!(first.finish(), second.finish());
    }

    /// An integer and the blob of its bytes must not digest alike, or a column whose class changed
    /// would slip through the reconstruction check.
    #[test]
    fn a_value_class_is_part_of_the_digest() {
        let integer = vec![Row::new(vec![Value::Integer(1)])];
        let blob = vec![Row::new(vec![Value::blob([0, 0, 0, 0, 0, 0, 0, 1])])];
        let mut first = DigestWriter::<Fnv1a128>::new();
        absorb_table(&mut first, "t", &integer);
        let mut second = DigestWriter::<Fnv1a128>::new();
        absorb_table(&mut second, "t", &blob);
        assert_ne!(first.finish(), second.finish());
    }

    /// A table that lost its last row and a table that never had it must differ.
    #[test]
    fn a_dropped_row_moves_the_digest() {
        let full = vec![
            Row::new(vec![Value::Integer(1)]),
            Row::new(vec![Value::Integer(2)]),
        ];
        let short = vec![Row::new(vec![Value::Integer(1)])];
        let mut first = DigestWriter::<Fnv1a128>::new();
        absorb_table(&mut first, "t", &full);
        let mut second = DigestWriter::<Fnv1a128>::new();
        absorb_table(&mut second, "t", &short);
        assert_ne!(first.finish(), second.finish());
    }
}
