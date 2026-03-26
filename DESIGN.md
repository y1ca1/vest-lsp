Vest‑DSL Language Server: Bare‑bones Implementation Plan

Background and Motivation

Binary parsing is notoriously error‑prone; manual parsers often contain buffer overflows or malformed handling.  The Vest project￼ formalizes binary formats in a safe DSL and generates memory‑safe, zero‑copy parsers.  To make this DSL productive, we need an interactive language server so that developers can write .vest files and get immediate feedback instead of waiting for a batch verifier.  The design document above outlined a highly advanced, reactive architecture.  This plan distills those ideas into a set of achievable milestones for building a minimal yet extensible language server in Rust using:
	•	Tree‑sitter: an incremental parsing library.  Its introduction notes that Tree‑sitter can build a concrete syntax tree (CST) and update it efficiently on every edit ￼ and aims to be fast enough to parse “on every keystroke” ￼ and robust in the presence of syntax errors ￼.  These properties let us respond to user edits without re‑parsing the entire file.
	•	Salsa: a query‑based incremental computation framework used in rust‑analyzer.  Salsa builds a dependency graph of pure functions (queries) and reuses cached results.  It implements an early‑cutoff optimisation so that if a change doesn’t affect a query’s result (e.g., adding whitespace), downstream queries remain cached.  Each query also tracks the revision when it last changed and the revision when it was last verified, enabling efficient invalidation.
	•	async‑lsp: an asynchronous LSP framework that builds on top of Tower.  Its documentation notes that it exposes an LspService core with a MainLoop driver and optional pluggable middleware layers for concurrency, panic handling, tracing, lifecycle management, and client monitoring ￼.  Developers can use a builder API or implement an omni LanguageServer trait to define handlers ￼.  This flexibility lets us integrate our own scheduling and incremental analysis logic while still handling the LSP protocol.

Simplified Architecture Overview

We still adopt the layered architecture described in the original document, but we implement it incrementally:
	1.	Source Database – maps URIs to file contents.  It records unsaved buffer overrides to allow analysis of in‑memory edits.
	2.	Parse Database – wraps the tree‑sitter parser.  It exposes queries like parse_file(file_id) that return a CST.  Because Tree‑sitter is incremental ￼, only edited ranges are re‑parsed.
	3.	Def Database / HIR – converts the CST into a simplified High‑level Intermediate Representation (HIR).  For the bare‑bones version, the HIR only needs to capture top‑level format definitions, field names, and references.
	4.	Analysis Queries – higher‑level queries (wire‑length computation, symbol resolution, etc.) built on top of the HIR.  These will be stubbed initially and added gradually.
	5.	LSP Layer – uses async‑lsp to serve JSON‑RPC requests.  async‑lsp’s LspService and MainLoop separate protocol handling from business logic.  We can compose middleware to control concurrency, tracing, and lifecycle, while our server code focuses on semantics.

Milestones

Zed Integration & Testing Strategy

Zed is a high‑performance editor with built‑in support for the Language Server Protocol (LSP) and Tree‑sitter. It obtains diagnostics from language servers and supports both push and pull variants of the LSP, making it compatible with any server ￼. This means we can run our language server within Zed and receive real‑time feedback as we edit .vest files. During development, we’ll configure a custom Vest language extension for Zed:
	•	Configure the language extension: create a languages/vest directory in a Zed extension and define a config.toml that points to our Tree‑sitter grammar. Register “Vest” as the language name.
	•	Register the language server: add an entry in extension.toml under [language_servers.vest-dsl] specifying the server’s name and the list of languages it applies to ￼. Then implement the language_server_command method to launch our compiled LSP executable ￼. Zed will automatically start the server when opening .vest files.
	•	Enable semantic tokens: in your Zed settings, set "semantic_tokens": "combined" to use both Tree‑sitter and LSP semantic highlighting, or "full" to rely exclusively on the LSP ￼.
	•	Define tasks: Zed’s tasks system lets you run shell commands in its integrated terminal. We will create tasks in .zed/tasks.json such as “build language server”, “run tree‑sitter tests”, and “cargo test” so that we can compile and test the project without leaving the editor. Tasks can reference environment variables like $ZED_FILE and $ZED_WORKTREE_ROOT ￼ and can be spawned or rerun via the command palette ￼.

Throughout development, we will use Zed to edit .vest files, view diagnostics from our server, and run tests via tasks. This interactive workflow helps catch errors early and iteratively refine the LSP.

Milestone 1 – Repository Setup & Grammar
	•	Create a Cargo workspace with separate crates for vest_lsp (language server), vest_syntax (tree‑sitter grammar) and vest_db (Salsa databases).
	•	Define the grammar in vest_syntax: reuse or adapt the existing Vest grammar from the Vest repository.  Use Tree‑sitter’s Grammar DSL (functions like seq, choice, repeat and the $ object to reference symbols) ￼.  Write tests using example .vest files in the repository.
	•	Generate parser: compile the grammar with the tree-sitter CLI and expose a Rust wrapper using the tree-sitter crate.
	•	Grammar tests: write grammar tests using Tree‑sitter’s built‑in testing framework. Each test case consists of a unique name, the source code, and an S‑expression representing the expected parse tree ￼. Place these files under test/corpus/ and run them with tree-sitter test.
	•	Zed integration: configure Zed’s language extension to use the Vest Tree‑sitter grammar for syntax highlighting. Create a task to run the grammar tests so you can quickly iterate within Zed.

Milestone 2 – Minimal LSP Skeleton
	•	Add async-lsp to vest_lsp and implement a basic LSP service.  async‑lsp lets you define handlers either via a builder API or by implementing the omni LanguageServer trait ￼.  Start with the builder: register handlers for initialize, initialized, shutdown, hover, and textDocument/completion.
	•	In initialize, advertise capabilities for incremental text synchronization, basic diagnostics and semantic tokens.
	•	Implement handlers for didOpen, didChange and didClose.  Maintain a SourceDatabase that stores file contents and unsaved edits.
	•	For each didChange, call the tree‑sitter parser on the edited file.  Convert parse errors into diagnostic messages sent via publishDiagnostics.
	•	Implement syntax highlighting by walking the CST and assigning token types (keywords, identifiers, numbers).  Use LSP’s semantic tokens if possible; at this stage, lexical highlighting suffices.
	•	Interactive testing in Zed: use your Zed extension to launch the LSP skeleton and open .vest files. Zed will display diagnostics produced by your server; because it supports both push and pull LSP variants, your diagnostics will appear automatically ￼.
	•	Unit tests: write unit tests for the SourceDatabase and initial handlers. Implement a test harness similar to the one described in the Salsa tutorial: create a database, set the input text, run the parser, and verify the result ￼. Use the expect-test crate to snapshot and compare outputs; the expect! macro stores the expected output and can be automatically updated with the UPDATE_EXPECT=1 environment variable ￼.

Milestone 3 – Incremental Query Infrastructure (Salsa)
	•	Create vest_db and add salsa as a dependency.  Define an input query source_text(file_id) and a tracked query parse_file(file_id) that returns the CST.  Salsa will cache parse trees and, thanks to early‑cutoff, will avoid recomputing downstream queries when whitespace/comments change.
	•	Add revision tracking: every didChange call increments the global revision and updates the relevant source_text input.  The LSP layer can then query the Salsa database for updated results.
	•	Provide a Lookup or FileId mapping from URIs to numeric IDs.  This can be a simple HashMap for now; more complex VFS/ID interning can be added later.
	•	Tests for incremental queries: write unit tests for the parse_file query and revision tracking. Use the parse_string helper from the Salsa tutorial ￼ to create a database, set source text, invoke parse_file, and check that incremental updates behave as expected.

Milestone 4 – Lowering to HIR & Symbol Resolution
	•	Implement a lower_to_hir(file_id) query that traverses the CST and builds a minimal HIR:
	•	Capture top‑level format declarations, their names, and the sequence of fields.
	•	Record dependent variables (e.g., @len) and alias references.
	•	Intern identifiers and types to u32 IDs to avoid repeated string allocations.  Use Salsa’s interning API for this.
	•	Build a basic symbol table: map names to definitions within a file.  Provide a query resolve_symbol(file_id, name) that returns the definition location if found.
	•	Extend the LSP layer to implement Go to Definition: on receiving a definition request, locate the CST node under the cursor, find its HIR node, and use resolve_symbol to jump to the definition.
	•	Tests for lowering and resolution: write tests that feed example .vest snippets through lower_to_hir and verify that the HIR matches expected structures. Use expect-test or plain assertions to compare symbol tables and resolution results.

Milestone 5 – Basic Formatting
	•	Implement a format_file(file_id) function that walks the CST and produces a formatted string: normalize whitespace around |, ensure consistent indentation and trailing commas.  Compare the original text to the formatted result and return a list of TextEdit operations.
	•	Hook this up to textDocument/formatting in the LSP layer.
	•	Formatter tests: provide test cases that take unformatted .vest inputs and verify that format_file returns the expected edits. Use expect-test to snapshot the formatted output or compare against fixture strings.

Milestone 6 – Hover Information & Wire Lengths
	•	Develop a query compute_wire_length(format_id) that returns either an exact byte length, or a range (with an algebraic expression involving dependent variables).  Handle all possible formats including primitive ones (u8, u16, fixed‑length arrays, etc.), as well as composed formats like struct with dependent fields, repetitions, and choices.
	•	Provide a hover_info(node_id) query that assembles:
  	•	The definition of the format/the format of a field/the definition of the enum discriminant/the constant value for a const field or const format/etc.
  	•	Wire‑length information (e.g., `wire length = 2` for a `u16`, `wire length = N` for `[u8; N]`, `wire length = 40` for `msg = { header: [u8; 16], body: [u8; 24] }`, `min wire length = 1, max wire length = 241` for `msg = { @len: u8 | 0..0xf0, payload: [u8; @len] }`, etc.).
  	•	Any comments immediately above the declaration.
    •	Don't show anything if hovering over comments or whitespace.
	•	In the LSP hover handler, call hover_info and format the result as Markdown.
	•	Hover tests: create unit tests for compute_wire_length and hover_info using a range of formats (primitives, arrays, choices). Assert that the returned size expressions and Markdown strings match expectations. Use Zed’s hover tooltips to manually verify that the server returns useful hover information.

Milestone 7 – Symbol References & Navigation
	•	Implement a references(symbol_id) query that collects all usages of a symbol across the file.  This requires scanning the HIR.
	•	Add textDocument/references support in the LSP: return a list of Locations for each occurrence.
	•	Implement renaming by updating the source text through TextEdits; ensure that only exact matches within the appropriate scopes are replaced.
	•	Navigation tests: test references and renaming functions in the HIR. Provide small .vest inputs where symbols appear multiple times, and verify that the server returns the correct locations and applies renames consistently.

Milestone 8 – Advanced Diagnostics
	•	Left‑recursion detection – traverse the HIR to build a directed graph where nodes are formats and edges represent “first format consumed” relationships.  Detect cycles that do not consume non‑nullable terminals; flag these as left‑recursive (which top‑down parsers cannot handle).  Provide an LSP diagnostic with the chain of involved formats.  Details can follow the graph algorithm described in the original design; implement a simplified version first.
	•	Cycle detection in wire lengths – when computing wire lengths, track in‑progress computations; if a query re‑enters itself, report a cycle and produce a placeholder result instead of panicking.
	•	Malleability and ambiguity checks – for choose expressions, verify that refinement ranges are disjoint.  For dependent arrays, warn if the length expression is not bounded by a refinement.  Emit diagnostics where malleability or ambiguity is detected.
	•	Diagnostic tests: write dedicated tests for left‑recursion detection, cycle detection, and malleability analysis. Provide examples of .vest formats that should trigger each diagnostic and assert that the server reports the correct messages and highlights.

Milestone 9 – Concurrency and Cancellation (Optional for MVP)
	•	Compose the appropriate async‑lsp middleware to manage concurrency and cancellation.  The concurrency middleware creates a separate runtime for LSP requests and uses Tokio tasks for background work ￼.  Integrate this middleware to offload Salsa queries from the main I/O thread and to propagate cancellation tokens.
	•	A single‑threaded prototype suffices initially.  Only add concurrency once earlier milestones are stable.

Milestone 10 – Testing and Vest Integration
	•	Use the examples in the Vest repository as test suites.  Create .vest files with intentional errors to verify diagnostics.
	•	Write unit tests for each Salsa query (parsing, lowering, symbol resolution, wire length).  Because queries are pure, testing them is straightforward.
	•	Package the server and publish a VS Code extension for early adopters.  Gather feedback and iterate.

Conclusion

By following these milestones, we progressively build a functional language server for Vest‑DSL.  Tree‑sitter gives us fast, incremental parsing ￼, async‑lsp provides an extensible framework for LSP protocol handling with pluggable middleware ￼, and Salsa provides robust incremental recomputation with early cutoff to avoid unnecessary work.  Starting with basic parsing and highlighting and incrementally layering symbol resolution, formatting, hover information, and advanced diagnostics will produce a useful tool early on while keeping the path open for sophisticated features like malleability analysis and concurrency.
