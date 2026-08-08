/*
  appellation: field <module>
  authors: @FL03
*/

/// A [`Field`] is a mathematical structure that consists of a set of elements
/// along with two operations: addition and multiplication. These operations must satisfy
/// certain properties, such as associativity, commutativity, and distributivity.
pub trait Field {
    /// the type of element within the field
    type Elem;
}
