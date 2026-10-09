fn main() {
    embed_resource::compile("echowisp.rc", embed_resource::NONE).manifest_optional().unwrap();
}
