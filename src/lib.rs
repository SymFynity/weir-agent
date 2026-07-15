// Module declarations are added by later tasks as each module is created.

pub mod backend;
pub mod config;
pub mod forwarder;
pub mod state;
pub mod symfynity;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert_eq!(2 + 2, 4);
    }
}
