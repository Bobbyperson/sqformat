use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use std::path::{Path, PathBuf};

use sqfmt_lib::config::{FileConfig, Format};
use sqformat_lsp::{
    DeclarationKind, LexicalToken, OwnedDeclaration, OwnedDocumentSymbol, SemanticDocument,
    TOKEN_MODIFIERS, TOKEN_TYPES, TypeIdentity, VmTargets, analyze_document, full_document_range,
    is_valid_identifier, offset_at, semantic_document, semantic_tokens,
};
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

mod workspace;

use workspace::{IndexedFile, ResolvedType, WorkspaceIndex};

/// Member checks are workspace-wide, so a badly resolved file could otherwise flood the client.
const MAX_MEMBER_DIAGNOSTICS: usize = 100;
const MAX_DUPLICATE_DIAGNOSTICS: usize = 100;
const MAX_ARITY_DIAGNOSTICS: usize = 100;
const MAX_ARGUMENT_TYPE_DIAGNOSTICS: usize = 100;
const MAX_TYPE_DIAGNOSTICS: usize = 100;
const MAX_LINT_DIAGNOSTICS: usize = 100;

#[derive(Debug)]
struct Document {
    text: String,
    symbols: Vec<OwnedDocumentSymbol>,
    semantic: SemanticDocument,
    lint: sqfmt_lint::Analysis,
    /// Kept from the document's one analysis so a semantic-token request does not tokenize again.
    lexical: Vec<LexicalToken>,
    local_diagnostics: Vec<Diagnostic>,
    version: i32,
}

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: RwLock<HashMap<Uri, Document>>,
    workspace: RwLock<WorkspaceIndex>,
    published_lint_files: RwLock<HashSet<Uri>>,
    provide_formatting: AtomicBool,
    advisory_lints: AtomicBool,
    watch_files: AtomicBool,
    /// A config file the client named, used instead of discovering one.
    config_file: RwLock<Option<PathBuf>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializationOptions {
    #[serde(default = "default_true")]
    provide_formatting: bool,
    #[serde(default)]
    advisory_lints: bool,
    /// A config file to use instead of discovering one, which mirrors the CLI's `--config`.
    #[serde(default)]
    config_file: Option<String>,
    /// External Squirrel declarations to index without treating them as workspace projects.
    #[serde(default)]
    api_source_roots: Vec<PathBuf>,
}

fn default_true() -> bool {
    true
}

impl Default for InitializationOptions {
    fn default() -> Self {
        Self {
            provide_formatting: true,
            advisory_lints: false,
            config_file: None,
            api_source_roots: Vec::new(),
        }
    }
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: RwLock::new(HashMap::new()),
            workspace: RwLock::new(WorkspaceIndex::default()),
            published_lint_files: RwLock::new(HashSet::new()),
            provide_formatting: AtomicBool::new(true),
            advisory_lints: AtomicBool::new(false),
            watch_files: AtomicBool::new(false),
            config_file: RwLock::new(None),
        }
    }

    /// Syntax and per-document checks, plus the checks that need the workspace index.
    fn diagnostics(
        workspace: &WorkspaceIndex,
        uri: &Uri,
        text: &str,
        semantic: &SemanticDocument,
        local_diagnostics: &[Diagnostic],
        lint_diagnostics: &[Diagnostic],
    ) -> Vec<Diagnostic> {
        let mut diagnostics = local_diagnostics.to_vec();
        diagnostics.extend(
            workspace
                .duplicate_declaration_diagnostics(uri, text, semantic)
                .into_iter()
                .take(MAX_DUPLICATE_DIAGNOSTICS),
        );
        diagnostics.extend(
            workspace
                .invalid_member_diagnostics(uri, text, semantic)
                .into_iter()
                .take(MAX_MEMBER_DIAGNOSTICS),
        );
        diagnostics.extend(
            workspace
                .call_arity_diagnostics(uri, text, semantic)
                .into_iter()
                .take(MAX_ARITY_DIAGNOSTICS),
        );
        diagnostics.extend(
            workspace
                .call_argument_type_diagnostics(uri, text, semantic)
                .into_iter()
                .take(MAX_ARGUMENT_TYPE_DIAGNOSTICS),
        );
        diagnostics.extend(
            workspace
                .type_mismatch_diagnostics(uri, text, semantic)
                .into_iter()
                .take(MAX_TYPE_DIAGNOSTICS),
        );
        diagnostics.extend(lint_diagnostics.iter().take(MAX_LINT_DIAGNOSTICS).cloned());
        diagnostics
    }

    async fn publish_all_diagnostics(&self) {
        let (mut publications, current_lint_files, open_files) = {
            let documents = self.documents.read().expect("document lock poisoned");
            let workspace = self.workspace.read().expect("workspace lock poisoned");
            let open_files = documents.keys().cloned().collect::<HashSet<_>>();
            let mut lint_publications = workspace
                .lint_diagnostic_publications(
                    self.advisory_lints.load(Ordering::Relaxed),
                    &open_files,
                )
                .into_iter()
                .collect::<HashMap<_, _>>();
            let current_lint_files = lint_publications.keys().cloned().collect::<HashSet<_>>();
            let mut publications = documents
                .iter()
                .map(|(uri, document)| {
                    let lint_diagnostics = lint_publications.remove(uri).unwrap_or_default();
                    let diagnostics = if workspace.is_api_file(uri) {
                        Vec::new()
                    } else {
                        Self::diagnostics(
                            &workspace,
                            uri,
                            &document.text,
                            &document.semantic,
                            &document.local_diagnostics,
                            &lint_diagnostics,
                        )
                    };
                    (uri.clone(), diagnostics, Some(document.version))
                })
                .collect::<Vec<_>>();
            publications.extend(
                lint_publications
                    .into_iter()
                    .map(|(uri, diagnostics)| (uri, diagnostics, None)),
            );
            (publications, current_lint_files, open_files)
        };
        let stale_lint_files = {
            let mut published = self
                .published_lint_files
                .write()
                .expect("published lint lock poisoned");
            let stale = published
                .difference(&current_lint_files)
                .filter(|uri| !open_files.contains(*uri))
                .cloned()
                .collect::<Vec<_>>();
            *published = current_lint_files;
            stale
        };
        publications.extend(
            stale_lint_files
                .into_iter()
                .map(|uri| (uri, Vec::new(), None)),
        );
        publications.sort_by(|left, right| left.0.cmp(&right.0));
        for (uri, diagnostics, version) in publications {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
    }

    /// The settings for this document: the config file the client named, or the nearest
    /// `.sqformat.toml` above it. Discovery lives in `sqfmt_lib::config`, so the server and the CLI
    /// find the same file the same way. A config that cannot be read is reported and the defaults
    /// are used, because refusing to format at all is worse than formatting with them.
    async fn discovered_format(&self, uri: &Uri) -> Format {
        let named = self
            .config_file
            .read()
            .expect("config lock poisoned")
            .clone();
        let format = match named {
            Some(path) => FileConfig::read(&path).and_then(|file| file.apply(Format::default())),
            None => match uri
                .to_file_path()
                .and_then(|path| path.parent().map(Path::to_path_buf))
            {
                Some(directory) => sqfmt_lib::config::discover(&directory),
                None => return Format::default(),
            },
        };
        match format {
            Ok(format) => format,
            Err(error) => {
                self.client
                    .log_message(MessageType::ERROR, error.to_string())
                    .await;
                Format::default()
            }
        }
    }

    fn index_open_document(
        &self,
        uri: Uri,
        text: &str,
        symbols: Vec<OwnedDocumentSymbol>,
        semantic: SemanticDocument,
        lint: sqfmt_lint::Analysis,
    ) {
        self.workspace
            .write()
            .expect("workspace lock poisoned")
            .insert(
                uri,
                IndexedFile::new(text.to_string(), symbols, semantic, lint),
            );
    }

    fn reload_disk_document(&self, uri: &Uri) {
        let indexed = IndexedFile::read(uri);
        let mut workspace = self.workspace.write().expect("workspace lock poisoned");
        match indexed {
            Some(indexed) => workspace.insert(uri.clone(), indexed),
            None => workspace.remove(uri),
        }
    }
}

impl LanguageServer for Backend {
    #[allow(deprecated)]
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let folders = params.workspace_folders.unwrap_or_else(|| {
            params
                .root_uri
                .map(|uri| {
                    let name = uri
                        .to_file_path()
                        .and_then(|path| {
                            path.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                        })
                        .unwrap_or_else(|| "workspace".to_string());
                    vec![WorkspaceFolder { uri, name }]
                })
                .unwrap_or_default()
        });
        let options = params
            .initialization_options
            .and_then(|value| serde_json::from_value::<InitializationOptions>(value).ok())
            .unwrap_or_default();
        self.provide_formatting
            .store(options.provide_formatting, Ordering::Relaxed);
        self.advisory_lints
            .store(options.advisory_lints, Ordering::Relaxed);
        *self.config_file.write().expect("config lock poisoned") =
            options.config_file.map(PathBuf::from);
        // Only clients that advertise dynamic registration are guaranteed to answer
        // `client/registerCapability`; an unanswered request would leave `initialized` pending.
        self.watch_files.store(
            params
                .capabilities
                .workspace
                .as_ref()
                .and_then(|workspace| workspace.did_change_watched_files.as_ref())
                .and_then(|watched| watched.dynamic_registration)
                .unwrap_or(false),
            Ordering::Relaxed,
        );
        {
            let mut workspace = self.workspace.write().expect("workspace lock poisoned");
            workspace.set_folders(folders);
            workspace.set_api_source_roots(options.api_source_roots);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: options
                    .provide_formatting
                    .then_some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![")".to_string()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: TOKEN_TYPES.to_vec(),
                                token_modifiers: TOKEN_MODIFIERS.to_vec(),
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                        },
                    ),
                ),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "sqformat".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            offset_encoding: None,
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let indexed = self
            .workspace
            .write()
            .expect("workspace lock poisoned")
            .scan();
        let open_documents = self
            .documents
            .read()
            .expect("document lock poisoned")
            .iter()
            .map(|(uri, document)| {
                (
                    uri.clone(),
                    IndexedFile::new(
                        document.text.clone(),
                        document.symbols.clone(),
                        document.semantic.clone(),
                        document.lint.clone(),
                    ),
                )
            })
            .collect::<Vec<_>>();
        {
            let mut workspace = self.workspace.write().expect("workspace lock poisoned");
            for (uri, document) in open_documents {
                workspace.insert(uri, document);
            }
        }
        if self.watch_files.load(Ordering::Relaxed) {
            let registration = Registration {
                id: "sqformat-watch-squirrel-files".to_string(),
                method: "workspace/didChangeWatchedFiles".to_string(),
                register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                    watchers: vec![
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/*.{nut,gnut}".to_string()),
                            kind: None,
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/mod.json".to_string()),
                            kind: None,
                        },
                    ],
                })
                .ok(),
            };
            if let Err(error) = self.client.register_capability(vec![registration]).await {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("failed to register workspace file watcher: {error}"),
                    )
                    .await;
            }
        } else {
            self.client
                .log_message(
                    MessageType::INFO,
                    "client does not support dynamic file watching; external changes need a restart",
                )
                .await;
        }
        self.client
            .log_message(
                MessageType::INFO,
                format!("sqformat language server initialized ({indexed} files indexed)"),
            )
            .await;
        self.publish_all_diagnostics().await;
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let documents = self.documents.read().expect("document lock poisoned");
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let workspace = self.workspace.read().expect("workspace lock poisoned");
        let tokens = semantic_tokens(
            &document.text,
            &document.lexical,
            &document.semantic,
            workspace.file_targets(&uri),
            &|name| workspace.global_declaration_kind(name),
        );
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        let analysis = analyze_document(&document.text);
        let local_diagnostics = analysis.diagnostics(&document.uri, &document.text);
        self.documents
            .write()
            .expect("document lock poisoned")
            .insert(
                document.uri.clone(),
                Document {
                    text: document.text.clone(),
                    symbols: analysis.symbols.clone(),
                    semantic: analysis.semantic.clone(),
                    lint: analysis.lint.clone(),
                    lexical: analysis.lexical,
                    local_diagnostics,
                    version: document.version,
                },
            );
        self.index_open_document(
            document.uri.clone(),
            &document.text,
            analysis.symbols,
            analysis.semantic,
            analysis.lint,
        );
        self.publish_all_diagnostics().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let analysis = analyze_document(&change.text);
        let local_diagnostics = analysis.diagnostics(&uri, &change.text);
        self.documents
            .write()
            .expect("document lock poisoned")
            .insert(
                uri.clone(),
                Document {
                    text: change.text.clone(),
                    symbols: analysis.symbols.clone(),
                    semantic: analysis.semantic.clone(),
                    lint: analysis.lint.clone(),
                    lexical: analysis.lexical,
                    local_diagnostics,
                    version,
                },
            );
        self.index_open_document(
            uri.clone(),
            &change.text,
            analysis.symbols,
            analysis.semantic,
            analysis.lint,
        );
        self.publish_all_diagnostics().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents
            .write()
            .expect("document lock poisoned")
            .remove(&uri);
        self.reload_disk_document(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
        self.publish_all_diagnostics().await;
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        if !self.provide_formatting.load(Ordering::Relaxed) {
            return Ok(None);
        }

        let uri = params.text_document.uri;
        let source = self
            .documents
            .read()
            .expect("document lock poisoned")
            .get(&uri)
            .map(|document| document.text.clone());
        let Some(source) = source else {
            return Ok(None);
        };

        let format = self.discovered_format(&uri).await;
        match sqfmt_lib::format_source(&source, format) {
            Ok(formatted) if formatted != source => Ok(Some(vec![TextEdit::new(
                full_document_range(&source),
                formatted,
            )])),
            Ok(_) => Ok(Some(Vec::new())),
            Err(error) => {
                self.client.log_message(MessageType::ERROR, error).await;
                Ok(None)
            }
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let document = self
            .documents
            .read()
            .expect("document lock poisoned")
            .get(&params.text_document.uri)
            .map(|document| (document.text.clone(), document.symbols.clone()));
        Ok(document.map(|(source, symbols)| {
            DocumentSymbolResponse::Nested(
                symbols
                    .into_iter()
                    .map(|symbol| symbol.into_lsp(&source))
                    .collect(),
            )
        }))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        let symbols = self
            .workspace
            .read()
            .expect("workspace lock poisoned")
            .query(&params.query, 100);
        Ok(Some(WorkspaceSymbolResponse::Nested(symbols)))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let source = self
            .documents
            .read()
            .expect("document lock poisoned")
            .get(&uri)
            .map(|document| document.text.clone());
        let Some(source) = source else {
            return Ok(None);
        };
        let Some(offset) = offset_at(&source, position) else {
            return Ok(None);
        };
        if let Some((semantic, receiver)) = member_completion_probe(&source, offset) {
            let workspace = self.workspace.read().expect("workspace lock poisoned");
            let Some(owner) =
                workspace.resolve_value_owner_with_document(&uri, &semantic, &receiver)
            else {
                return Ok(Some(CompletionResponse::Array(Vec::new())));
            };
            let targets = semantic
                .targets_at(offset)
                .intersection(workspace.file_targets(&uri));
            let mut names = HashSet::new();
            let items = workspace
                .members_for_type_at_with_document(&owner, Some(&uri), Some(&semantic), offset)
                .into_iter()
                .filter(|member| member.targets.compatible_with(targets))
                .filter(|member| names.insert(member.name.clone()))
                .map(member_completion_item)
                .collect();
            return Ok(Some(CompletionResponse::Array(items)));
        }
        let file_targets = self
            .workspace
            .read()
            .expect("workspace lock poisoned")
            .file_targets(&uri);
        let (mut items, mut names, targets) = {
            let documents = self.documents.read().expect("document lock poisoned");
            let Some(document) = documents.get(&uri) else {
                return Ok(None);
            };
            let Some(offset) = offset_at(&document.text, position) else {
                return Ok(None);
            };
            if completion_follows_dot(&document.text, offset) {
                let Some(reference) =
                    document
                        .semantic
                        .member_references
                        .iter()
                        .find(|reference| {
                            reference.range.start <= offset && offset <= reference.range.end
                        })
                else {
                    return Ok(Some(CompletionResponse::Array(Vec::new())));
                };
                let workspace = self.workspace.read().expect("workspace lock poisoned");
                let Some(owner) = workspace.resolve_value_owner(&uri, &reference.receiver) else {
                    return Ok(Some(CompletionResponse::Array(Vec::new())));
                };
                let targets = document
                    .semantic
                    .targets_at(offset)
                    .intersection(file_targets);
                let mut names = HashSet::new();
                let items = workspace
                    .members_for_type_at_with_document(&owner, Some(&uri), None, offset)
                    .into_iter()
                    .filter(|member| member.targets.compatible_with(targets))
                    .filter(|member| names.insert(member.name.clone()))
                    .map(member_completion_item)
                    .collect();
                return Ok(Some(CompletionResponse::Array(items)));
            }

            let targets = document
                .semantic
                .targets_at(offset)
                .intersection(file_targets);
            let mut names = HashSet::new();
            let items = document
                .semantic
                .visible_declarations(offset)
                .into_iter()
                .filter(|declaration| declaration.targets.compatible_with(targets))
                .filter(|declaration| names.insert(declaration.name.clone()))
                .map(|declaration| completion_item(declaration, "0"))
                .collect::<Vec<_>>();
            (items, names, targets)
        };

        let globals = self
            .workspace
            .read()
            .expect("workspace lock poisoned")
            .globals();
        for declarations in globals.chunk_by(|left, right| left.name == right.name) {
            let declaration = &declarations[0];
            if !declarations
                .iter()
                .any(|declaration| declaration.targets.compatible_with(targets))
            {
                continue;
            }
            if names.insert(declaration.name.clone()) {
                let sort_text = format!("1{}", declaration.name);
                let mut details = declarations
                    .iter()
                    .map(|declaration| declaration.detail.as_str())
                    .collect::<Vec<_>>();
                details.dedup();
                items.push(CompletionItem {
                    label: declaration.name.clone(),
                    kind: Some(completion_kind(declaration.kind)),
                    detail: Some(details.join("\n")),
                    documentation: Some(completion_documentation(&details.join("\n\n"))),
                    sort_text: Some(sort_text),
                    ..Default::default()
                });
            }
        }
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let (source, cached_semantic) = {
            let documents = self.documents.read().expect("document lock poisoned");
            let Some(document) = documents.get(&uri) else {
                return Ok(None);
            };
            (document.text.clone(), document.semantic.clone())
        };
        let Some(offset) = offset_at(&source, position) else {
            return Ok(None);
        };
        let Some((semantic, call)) = cached_semantic
            .call_at(offset)
            .cloned()
            .map(|call| (cached_semantic, call))
            .or_else(|| signature_help_probe(&source, offset))
        else {
            return Ok(None);
        };
        let signatures = self
            .workspace
            .read()
            .expect("workspace lock poisoned")
            .callable_signatures_with_document(&uri, &semantic, &call.callable);
        if signatures.is_empty() {
            return Ok(None);
        }
        let active_signature = params
            .context
            .and_then(|context| context.active_signature_help)
            .and_then(|help| help.active_signature)
            .filter(|index| (*index as usize) < signatures.len())
            .unwrap_or(0);
        let signature = &signatures[active_signature as usize];
        let active_parameter = active_parameter(&call, offset, signature);
        Ok(Some(SignatureHelp {
            signatures: signatures
                .into_iter()
                .map(|signature| SignatureInformation {
                    label: signature.label,
                    documentation: None,
                    parameters: Some(
                        signature
                            .parameters
                            .into_iter()
                            .map(|parameter| ParameterInformation {
                                label: ParameterLabel::Simple(parameter.label),
                                documentation: None,
                            })
                            .collect(),
                    ),
                    active_parameter: None,
                })
                .collect(),
            active_signature: Some(active_signature),
            active_parameter,
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let file_targets = self
            .workspace
            .read()
            .expect("workspace lock poisoned")
            .file_targets(&uri);
        let target = {
            let documents = self.documents.read().expect("document lock poisoned");
            let Some(document) = documents.get(&uri) else {
                return Ok(None);
            };
            let Some(offset) = offset_at(&document.text, position) else {
                return Ok(None);
            };

            let targets = document
                .semantic
                .targets_at(offset)
                .intersection(file_targets);
            if let Some(member) = resolve_member(document, position) {
                Some(DefinitionTarget::Member(member))
            } else if let Some(declaration) = document.semantic.declaration_at(offset) {
                Some(DefinitionTarget::Location(location_for_range(
                    uri.clone(),
                    &document.text,
                    declaration.range.clone(),
                )))
            } else if let Some(reference) = document.semantic.reference_at(offset) {
                match &reference.target {
                    Some(range) => Some(DefinitionTarget::Location(location_for_range(
                        uri.clone(),
                        &document.text,
                        range.clone(),
                    ))),
                    None => Some(DefinitionTarget::Workspace(reference.name.clone(), targets)),
                }
            } else {
                None
            }
        };

        match target {
            Some(DefinitionTarget::Location(location)) => {
                Ok(Some(GotoDefinitionResponse::Scalar(location)))
            }
            Some(DefinitionTarget::Workspace(name, targets)) => {
                let locations = self
                    .workspace
                    .read()
                    .expect("workspace lock poisoned")
                    .definitions(&name, targets);
                match locations.as_slice() {
                    [] => Ok(None),
                    [location] => Ok(Some(GotoDefinitionResponse::Scalar(location.clone()))),
                    _ => Ok(Some(GotoDefinitionResponse::Array(locations))),
                }
            }
            Some(DefinitionTarget::Member(member)) => {
                let workspace = self.workspace.read().expect("workspace lock poisoned");
                let Some(owner) = member_owner(&workspace, &uri, &member.owner, &member.name)
                else {
                    return Ok(None);
                };
                let locations = workspace
                    .member_declarations_for_type(&owner, &member.name)
                    .into_iter()
                    .map(|member| Location::new(member.uri, member.range))
                    .collect::<Vec<_>>();
                match locations.as_slice() {
                    [] => Ok(None),
                    [location] => Ok(Some(GotoDefinitionResponse::Scalar(location.clone()))),
                    _ => Ok(Some(GotoDefinitionResponse::Array(locations))),
                }
            }
            None => Ok(None),
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let member = {
            let documents = self.documents.read().expect("document lock poisoned");
            let Some(document) = documents.get(&uri) else {
                return Ok(None);
            };
            resolve_member(document, position)
        };
        if let Some(member) = member {
            let workspace = self.workspace.read().expect("workspace lock poisoned");
            let Some(owner) = member_owner(&workspace, &uri, &member.owner, &member.name) else {
                return Ok(None);
            };
            let mut details = workspace
                .member_declarations_for_type(&owner, &member.name)
                .into_iter()
                .map(|member| member.detail)
                .collect::<Vec<_>>();
            details.dedup();
            if details.is_empty() {
                return Ok(None);
            }
            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: details
                        .into_iter()
                        .map(|detail| format!("```squirrel\n{detail}\n```"))
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                }),
                range: Some(member.selection),
            }));
        }

        let resolved = {
            let documents = self.documents.read().expect("document lock poisoned");
            let document = documents.get(&uri).expect("document remained open");
            resolve_symbol(document, position)
        };
        let Some(resolved) = resolved else {
            return Ok(None);
        };

        let details = match &resolved.target {
            SymbolTarget::Local(range) => {
                let documents = self.documents.read().expect("document lock poisoned");
                let document = documents.get(&uri).expect("document remained open");
                document
                    .semantic
                    .declaration_for_range(range)
                    .map(|declaration| vec![declaration.detail.clone()])
                    .unwrap_or_default()
            }
            SymbolTarget::Global(name) => self
                .workspace
                .read()
                .expect("workspace lock poisoned")
                .global_declarations(name)
                .into_iter()
                .map(|declaration| declaration.detail)
                .collect(),
        };
        let mut details = details;
        details.dedup();
        if details.is_empty() {
            return Ok(None);
        }

        let value = details
            .into_iter()
            .map(|detail| format!("```squirrel\n{detail}\n```"))
            .collect::<Vec<_>>()
            .join("\n\n");
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(resolved.selection),
        }))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let member = {
            let documents = self.documents.read().expect("document lock poisoned");
            let Some(document) = documents.get(&uri) else {
                return Ok(None);
            };
            resolve_member(document, position)
        };
        if let Some(member) = member {
            let workspace = self.workspace.read().expect("workspace lock poisoned");
            let Some(owner) = member_owner(&workspace, &uri, &member.owner, &member.name) else {
                return Ok(None);
            };
            let locations = workspace
                .member_occurrences_for_type(&owner, &member.name)
                .into_iter()
                .filter(|occurrence| params.context.include_declaration || !occurrence.declaration)
                .map(|occurrence| Location::new(occurrence.uri, occurrence.range))
                .collect();
            return Ok(Some(locations));
        }

        let resolved = {
            let documents = self.documents.read().expect("document lock poisoned");
            let document = documents.get(&uri).expect("document remained open");
            resolve_symbol(document, position)
        };

        let Some(resolved) = resolved else {
            return Ok(None);
        };
        match resolved.target {
            SymbolTarget::Local(declaration) => {
                let documents = self.documents.read().expect("document lock poisoned");
                let document = documents.get(&uri).expect("document remained open");
                let mut locations = document
                    .semantic
                    .local_references(&declaration)
                    .into_iter()
                    .map(|range| location_for_range(uri.clone(), &document.text, range))
                    .collect::<Vec<_>>();
                if params.context.include_declaration {
                    locations.insert(0, location_for_range(uri, &document.text, declaration));
                }
                Ok(Some(locations))
            }
            SymbolTarget::Global(name) => {
                let locations = self
                    .workspace
                    .read()
                    .expect("workspace lock poisoned")
                    .global_occurrences(&name)
                    .into_iter()
                    .filter(|occurrence| {
                        params.context.include_declaration || !occurrence.declaration
                    })
                    .map(|occurrence| Location::new(occurrence.uri, occurrence.range))
                    .collect();
                Ok(Some(locations))
            }
        }
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        if self
            .workspace
            .read()
            .expect("workspace lock poisoned")
            .is_api_file(&params.text_document.uri)
        {
            return Ok(None);
        }
        let member = {
            let documents = self.documents.read().expect("document lock poisoned");
            let Some(document) = documents.get(&params.text_document.uri) else {
                return Ok(None);
            };
            resolve_member(document, params.position)
        };
        if let Some(member) = member {
            let workspace = self.workspace.read().expect("workspace lock poisoned");
            let Some(owner) = member_owner(
                &workspace,
                &params.text_document.uri,
                &member.owner,
                &member.name,
            ) else {
                return Ok(None);
            };
            let declarations = workspace.member_declarations_for_type(&owner, &member.name);
            if declarations.len() != 1
                || workspace.is_api_file(&declarations[0].uri)
                || workspace
                    .member_occurrences_for_type(&owner, &member.name)
                    .iter()
                    .any(|occurrence| workspace.is_api_file(&occurrence.uri))
            {
                return Ok(None);
            }
            return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                range: member.selection,
                placeholder: member.name,
            }));
        }

        let resolved = {
            let documents = self.documents.read().expect("document lock poisoned");
            let document = documents
                .get(&params.text_document.uri)
                .expect("document remained open");
            resolve_symbol(document, params.position)
        };
        let Some(resolved) = resolved else {
            return Ok(None);
        };

        if let SymbolTarget::Global(name) = &resolved.target {
            let workspace = self.workspace.read().expect("workspace lock poisoned");
            let declaration_count = workspace
                .global_occurrences(name)
                .into_iter()
                .filter(|occurrence| {
                    occurrence.declaration && !workspace.is_api_file(&occurrence.uri)
                })
                .count();
            let has_api_occurrence = workspace
                .global_occurrences(name)
                .iter()
                .any(|occurrence| workspace.is_api_file(&occurrence.uri));
            if declaration_count == 0 || has_api_occurrence {
                return Ok(None);
            }
        }

        Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
            range: resolved.selection,
            placeholder: resolved.name,
        }))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        if !is_valid_identifier(&params.new_name) {
            return Err(tower_lsp_server::jsonrpc::Error::invalid_params(
                "new name must be a valid non-keyword Squirrel identifier",
            ));
        }

        let uri = params.text_document_position.text_document.uri;
        if self
            .workspace
            .read()
            .expect("workspace lock poisoned")
            .is_api_file(&uri)
        {
            return Err(tower_lsp_server::jsonrpc::Error::invalid_params(
                "cannot rename symbols in an API source root",
            ));
        }
        let mut changes = HashMap::<Uri, Vec<TextEdit>>::new();
        let member = {
            let documents = self.documents.read().expect("document lock poisoned");
            let Some(document) = documents.get(&uri) else {
                return Ok(None);
            };
            resolve_member(document, params.text_document_position.position)
        };
        if let Some(member) = member {
            let workspace = self.workspace.read().expect("workspace lock poisoned");
            let Some(owner) = member_owner(&workspace, &uri, &member.owner, &member.name) else {
                return Ok(None);
            };
            let declarations = workspace.member_declarations_for_type(&owner, &member.name);
            if declarations.len() != 1 {
                return Err(tower_lsp_server::jsonrpc::Error::invalid_params(
                    "cannot rename an ambiguous member",
                ));
            }
            if workspace.is_api_file(&declarations[0].uri) {
                return Err(tower_lsp_server::jsonrpc::Error::invalid_params(
                    "cannot rename a member declared in an API source root",
                ));
            }
            if workspace
                .member_occurrences_for_type(&owner, &member.name)
                .iter()
                .any(|occurrence| workspace.is_api_file(&occurrence.uri))
            {
                return Err(tower_lsp_server::jsonrpc::Error::invalid_params(
                    "cannot rename a member referenced from an API source root",
                ));
            }
            if member.name != params.new_name
                && !workspace
                    .member_declarations_for_type(&owner, &params.new_name)
                    .is_empty()
            {
                return Err(tower_lsp_server::jsonrpc::Error::invalid_params(
                    "new name conflicts with an existing member",
                ));
            }
            for occurrence in workspace.member_occurrences_for_type(&owner, &member.name) {
                if workspace.is_api_file(&occurrence.uri) {
                    continue;
                }
                changes
                    .entry(occurrence.uri)
                    .or_default()
                    .push(TextEdit::new(occurrence.range, params.new_name.clone()));
            }
            drop(workspace);
            for edits in changes.values_mut() {
                edits.sort_by_key(|edit| Reverse(edit.range.start));
            }
            return Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }));
        }

        let resolved = {
            let documents = self.documents.read().expect("document lock poisoned");
            let document = documents.get(&uri).expect("document remained open");
            resolve_symbol(document, params.text_document_position.position)
        };
        let Some(resolved) = resolved else {
            return Ok(None);
        };

        match resolved.target {
            SymbolTarget::Local(declaration) => {
                let documents = self.documents.read().expect("document lock poisoned");
                let document = documents.get(&uri).expect("document remained open");
                let edits = changes.entry(uri).or_default();
                edits.push(TextEdit::new(
                    lsp_range(&document.text, declaration.clone()),
                    params.new_name.clone(),
                ));
                edits.extend(
                    document
                        .semantic
                        .local_references(&declaration)
                        .into_iter()
                        .map(|range| {
                            TextEdit::new(lsp_range(&document.text, range), params.new_name.clone())
                        }),
                );
            }
            SymbolTarget::Global(name) => {
                let occurrences = self
                    .workspace
                    .read()
                    .expect("workspace lock poisoned")
                    .global_occurrences(&name);
                let workspace = self.workspace.read().expect("workspace lock poisoned");
                if occurrences
                    .iter()
                    .any(|occurrence| workspace.is_api_file(&occurrence.uri))
                {
                    return Err(tower_lsp_server::jsonrpc::Error::invalid_params(
                        "cannot rename a symbol referenced from an API source root",
                    ));
                }
                if occurrences
                    .iter()
                    .filter(|occurrence| {
                        occurrence.declaration && !workspace.is_api_file(&occurrence.uri)
                    })
                    .count()
                    == 0
                {
                    return Err(tower_lsp_server::jsonrpc::Error::invalid_params(
                        "cannot rename an unresolved global symbol",
                    ));
                }
                for occurrence in occurrences {
                    if workspace.is_api_file(&occurrence.uri) {
                        continue;
                    }
                    changes
                        .entry(occurrence.uri)
                        .or_default()
                        .push(TextEdit::new(occurrence.range, params.new_name.clone()));
                }
            }
        }

        for edits in changes.values_mut() {
            edits.sort_by_key(|edit| Reverse(edit.range.start));
        }
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let open_documents: Vec<_> = self
            .documents
            .read()
            .expect("document lock poisoned")
            .keys()
            .cloned()
            .collect();
        {
            let mut workspace = self.workspace.write().expect("workspace lock poisoned");
            let mut manifests_changed = false;
            for change in params.changes {
                if change.uri.path().as_str().ends_with("mod.json") {
                    manifests_changed = true;
                    continue;
                }
                if open_documents.contains(&change.uri) {
                    continue;
                }
                if change.typ == FileChangeType::DELETED {
                    workspace.remove(&change.uri);
                } else if let Some(indexed) = IndexedFile::read(&change.uri) {
                    workspace.insert(change.uri, indexed);
                }
            }
            // A manifest change moves scripts between VMs, so the mapping is rebuilt wholesale.
            if manifests_changed {
                workspace.rescan_manifests();
            }
        }
        self.publish_all_diagnostics().await;
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        let open_documents: Vec<_> = self
            .documents
            .read()
            .expect("document lock poisoned")
            .iter()
            .map(|(uri, document)| {
                (
                    uri.clone(),
                    IndexedFile::new(
                        document.text.clone(),
                        document.symbols.clone(),
                        document.semantic.clone(),
                        document.lint.clone(),
                    ),
                )
            })
            .collect();
        {
            let mut workspace = self.workspace.write().expect("workspace lock poisoned");
            workspace.remove_folders(&params.event.removed);
            workspace.add_folders(params.event.added);
            workspace.scan();
            for (uri, document) in open_documents {
                workspace.insert(uri, document);
            }
        }
        self.publish_all_diagnostics().await;
    }
}

enum DefinitionTarget {
    Location(Location),
    Workspace(String, VmTargets),
    Member(ResolvedMember),
}

fn location_for_range(uri: Uri, source: &str, range: std::ops::Range<usize>) -> Location {
    Location::new(
        uri,
        tower_lsp_server::ls_types::Range::new(
            sqformat_lsp::position_at(source, range.start),
            sqformat_lsp::position_at(source, range.end),
        ),
    )
}

fn lsp_range(source: &str, range: std::ops::Range<usize>) -> Range {
    Range::new(
        sqformat_lsp::position_at(source, range.start),
        sqformat_lsp::position_at(source, range.end),
    )
}

fn completion_item(declaration: &OwnedDeclaration, sort_prefix: &str) -> CompletionItem {
    CompletionItem {
        label: declaration.name.clone(),
        kind: Some(completion_kind(declaration.kind)),
        detail: Some(declaration.detail.clone()),
        documentation: Some(completion_documentation(&declaration.detail)),
        sort_text: Some(format!("{sort_prefix}{}", declaration.name)),
        ..Default::default()
    }
}

fn member_completion_item(member: workspace::WorkspaceMember) -> CompletionItem {
    CompletionItem {
        label: member.name.clone(),
        kind: Some(completion_kind(member.kind)),
        detail: Some(member.detail.clone()),
        documentation: Some(completion_documentation(&member.detail)),
        sort_text: Some(format!("0{}", member.name)),
        ..Default::default()
    }
}

fn completion_kind(kind: DeclarationKind) -> CompletionItemKind {
    match kind {
        DeclarationKind::Function => CompletionItemKind::FUNCTION,
        DeclarationKind::Constructor => CompletionItemKind::CONSTRUCTOR,
        DeclarationKind::Class => CompletionItemKind::CLASS,
        DeclarationKind::Constant => CompletionItemKind::CONSTANT,
        DeclarationKind::Enum => CompletionItemKind::ENUM,
        DeclarationKind::Struct => CompletionItemKind::STRUCT,
        DeclarationKind::Type => CompletionItemKind::TYPE_PARAMETER,
        DeclarationKind::Variable | DeclarationKind::Parameter => CompletionItemKind::VARIABLE,
        DeclarationKind::Field => CompletionItemKind::FIELD,
        DeclarationKind::Method => CompletionItemKind::METHOD,
    }
}

fn completion_documentation(detail: &str) -> Documentation {
    Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: format!("```squirrel\n{detail}\n```"),
    })
}

fn completion_follows_dot(source: &str, offset: usize) -> bool {
    let before = &source[..offset];
    let prefix_start = before
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_alphanumeric() && *character != '_')
        .map_or(0, |(index, character)| index + character.len_utf8());
    source[..prefix_start].trim_end().ends_with('.')
}

fn member_completion_probe(
    source: &str,
    offset: usize,
) -> Option<(SemanticDocument, sqformat_lsp::ValueSource)> {
    let before = &source[..offset];
    let prefix_start = before
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_alphanumeric() && *character != '_')
        .map_or(0, |(index, character)| index + character.len_utf8());
    if !source[..prefix_start].trim_end().ends_with('.') {
        return None;
    }

    let original_len = offset - prefix_start;
    let mut sentinel = "__sqformat_completion".to_string();
    while source.contains(&sentinel) {
        sentinel.push('_');
    }
    let mut probe = source.to_string();
    if original_len == 0 {
        probe.insert_str(offset, &sentinel);
    } else {
        probe.replace_range(prefix_start..offset, &sentinel);
    }
    let semantic = semantic_document(&probe);
    let sentinel_range = prefix_start..prefix_start + sentinel.len();
    let reference = semantic
        .member_references
        .iter()
        .find(|reference| reference.range == sentinel_range && reference.name == sentinel)?;
    Some((semantic.clone(), reference.receiver.clone()))
}

fn signature_help_probe(
    source: &str,
    offset: usize,
) -> Option<(SemanticDocument, sqformat_lsp::OwnedCall)> {
    let mut sentinel = "__sqformat_signature".to_string();
    while source.contains(&sentinel) {
        sentinel.push('_');
    }
    for close_count in 1..=32 {
        let mut probe = source.to_string();
        let suffix = format!("{sentinel}{}", ")".repeat(close_count));
        probe.insert_str(offset, &suffix);
        let semantic = semantic_document(&probe);
        if semantic.declarations.is_empty() && semantic.calls.is_empty() {
            continue;
        }
        if let Some(call) = semantic.call_at(offset).cloned() {
            return Some((semantic, call));
        }
    }
    None
}

fn active_parameter(
    call: &sqformat_lsp::OwnedCall,
    offset: usize,
    signature: &sqformat_lsp::OwnedSignature,
) -> Option<u32> {
    if signature.parameters.is_empty() {
        return None;
    }
    let raw = call
        .commas
        .iter()
        .filter(|comma| comma.end <= offset)
        .count();
    let last = signature.parameters.len() - 1;
    Some(raw.min(last) as u32)
}

struct ResolvedSymbol {
    name: String,
    selection: Range,
    target: SymbolTarget,
}

struct ResolvedMember {
    name: String,
    selection: Range,
    owner: MemberOwner,
}

enum MemberOwner {
    Known(TypeIdentity),
    Receiver(sqformat_lsp::ValueSource),
}

enum SymbolTarget {
    Local(std::ops::Range<usize>),
    Global(String),
}

fn resolve_symbol(document: &Document, position: Position) -> Option<ResolvedSymbol> {
    let offset = offset_at(&document.text, position)?;
    if let Some(declaration) = document.semantic.declaration_at(offset) {
        let target = if declaration.is_global {
            SymbolTarget::Global(declaration.name.clone())
        } else {
            SymbolTarget::Local(declaration.range.clone())
        };
        return Some(ResolvedSymbol {
            name: declaration.name.clone(),
            selection: lsp_range(&document.text, declaration.range.clone()),
            target,
        });
    }

    let reference = document.semantic.reference_at(offset)?;
    let target = match &reference.target {
        Some(range) => match document.semantic.declaration_for_range(range) {
            Some(declaration) if declaration.is_global => {
                SymbolTarget::Global(declaration.name.clone())
            }
            _ => SymbolTarget::Local(range.clone()),
        },
        None => SymbolTarget::Global(reference.name.clone()),
    };
    Some(ResolvedSymbol {
        name: reference.name.clone(),
        selection: lsp_range(&document.text, reference.range.clone()),
        target,
    })
}

fn resolve_member(document: &Document, position: Position) -> Option<ResolvedMember> {
    let offset = offset_at(&document.text, position)?;
    if let Some(reference) = document.semantic.member_reference_at(offset) {
        return Some(ResolvedMember {
            name: reference.name.clone(),
            selection: lsp_range(&document.text, reference.range.clone()),
            owner: MemberOwner::Receiver(reference.receiver.clone()),
        });
    }

    if let Some(declaration) = document.semantic.declaration_at(offset)
        && let Some(owner) = &declaration.owner
    {
        return Some(ResolvedMember {
            name: declaration.name.clone(),
            selection: lsp_range(&document.text, declaration.range.clone()),
            owner: MemberOwner::Known(owner.clone()),
        });
    }

    let reference = document.semantic.reference_at(offset)?;
    let declaration = reference
        .target
        .as_ref()
        .and_then(|target| document.semantic.declaration_for_range(target))?;
    Some(ResolvedMember {
        name: reference.name.clone(),
        selection: lsp_range(&document.text, reference.range.clone()),
        owner: MemberOwner::Known(declaration.owner.clone()?),
    })
}

fn member_owner(
    workspace: &WorkspaceIndex,
    uri: &Uri,
    owner: &MemberOwner,
    name: &str,
) -> Option<ResolvedType> {
    let receiver = match owner {
        MemberOwner::Known(owner) => workspace.resolve_type_identity(uri, owner),
        MemberOwner::Receiver(receiver) => workspace.resolve_value_owner(uri, receiver),
    }?;
    workspace.member_owner_for_type(&receiver, name)
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
