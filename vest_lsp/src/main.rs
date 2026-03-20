#[tokio::main(flavor = "current_thread")]
async fn main() {
    vest_lsp::run_stdio_server()
        .await
        .expect("Vest LSP exited with an error");
}
