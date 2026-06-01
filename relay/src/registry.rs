pub struct Registry;
impl Registry {
    pub fn new() -> Self { Registry }
    pub async fn run(self, _public: &str, _agent: &str) -> anyhow::Result<()> { Ok(()) }
}
