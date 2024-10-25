fn main() {
    #[cfg(feature = "bundled")]
    {
        minijinja_embed::embed_templates!("templates");
    }
}
