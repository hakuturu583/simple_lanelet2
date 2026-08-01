//! `autoware_lanelet2_extension_python`, as far as it can be provided without ROS.
//!
//! These classes live in the same cdylib as `lanelet2` because PyO3 classes are
//! static Rust types and the subtype-to-class mapping has to reach them. What makes
//! the extension *optional* is not packaging but registration: nothing here touches
//! the registry until the Python shim calls `__register__`, so until it does, a map
//! carrying these subtypes fails exactly as stock Lanelet2 makes it fail.

pub mod regelem;
