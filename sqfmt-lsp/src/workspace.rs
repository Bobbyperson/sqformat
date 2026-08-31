use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use sqformat_lsp::{
    DeclarationKind, LoadOrder, OwnedDeclaration, OwnedDocumentSymbol, OwnedSignature, ScriptEntry,
    SemanticDocument, TypeIdentity, ValueSource, VmTargets, analyze_document, position_at,
    read_manifest, workspace_symbols,
};
use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticRelatedInformation, Location, NumberOrString, OneOf, Uri,
    WorkspaceFolder, WorkspaceSymbol,
};

#[derive(Clone, Debug)]
pub struct WorkspaceOccurrence {
    pub uri: Uri,
    pub range: tower_lsp_server::ls_types::Range,
    pub declaration: bool,
}

#[derive(Clone, Debug)]
pub struct WorkspaceDeclaration {
    pub uri: Uri,
    pub range: tower_lsp_server::ls_types::Range,
    pub name: String,
    pub kind: DeclarationKind,
    pub detail: String,
    pub targets: VmTargets,
}

#[derive(Clone, Debug)]
pub struct WorkspaceMember {
    pub uri: Uri,
    pub range: tower_lsp_server::ls_types::Range,
    pub name: String,
    pub kind: DeclarationKind,
    pub detail: String,
    pub targets: VmTargets,
}

pub type ResolvedType = sqfmt_lint::ResolvedType<Uri>;

#[derive(Debug)]
pub struct IndexedFile {
    source: String,
    symbols: Vec<OwnedDocumentSymbol>,
    semantic: SemanticDocument,
    lint: sqfmt_lint::Analysis,
}

impl IndexedFile {
    pub fn new(
        source: String,
        symbols: Vec<OwnedDocumentSymbol>,
        semantic: SemanticDocument,
        lint: sqfmt_lint::Analysis,
    ) -> Self {
        Self {
            source,
            symbols,
            semantic,
            lint,
        }
    }

    pub fn read(uri: &Uri) -> Option<Self> {
        let path = uri.to_file_path()?;
        let source = std::fs::read_to_string(path).ok()?;
        // Indexed files retain enough analysis for workspace diagnostics and lookups.
        let analysis = analyze_document(&source);
        Some(Self::new(
            source,
            analysis.symbols,
            analysis.semantic,
            analysis.lint,
        ))
    }
}

#[derive(Debug, Default)]
pub struct WorkspaceIndex {
    folders: Vec<WorkspaceFolder>,
    api_source_roots: Vec<PathBuf>,
    api_files: HashSet<Uri>,
    api_manifests: HashSet<Uri>,
    files: HashMap<Uri, IndexedFile>,
    /// Scripts a `mod.json` describes, by the file they name.
    scripts: HashMap<Uri, ScriptEntry>,
    manifests: HashMap<Uri, String>,
}

impl WorkspaceIndex {
    fn semantic_workspace(&self) -> sqfmt_lint::SemanticWorkspace<'_, Uri> {
        sqfmt_lint::SemanticWorkspace::new(self.files.iter().map(|(uri, file)| {
            sqfmt_lint::SemanticFile {
                id: uri.clone(),
                document: &file.semantic,
                targets: self.file_targets(uri),
            }
        }))
    }

    pub fn duplicate_declaration_diagnostics(
        &self,
        uri: &Uri,
        source: &str,
        semantic: &SemanticDocument,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self.semantic_rule_diagnostics(
            uri,
            source,
            semantic,
            sqfmt_lint::DUPLICATE_DECLARATION_RULE,
        );
        for diagnostic in &mut diagnostics {
            if let Some(duplicate) = semantic
                .duplicates
                .iter()
                .find(|duplicate| lsp_range(source, duplicate.range.clone()) == diagnostic.range)
            {
                diagnostic.related_information = Some(vec![DiagnosticRelatedInformation {
                    location: Location::new(
                        uri.clone(),
                        lsp_range(source, duplicate.previous.clone()),
                    ),
                    message: format!("`{}` is first declared here", duplicate.name),
                }]);
            }
        }
        diagnostics
    }

    fn semantic_rule_diagnostics(
        &self,
        uri: &Uri,
        source: &str,
        semantic: &SemanticDocument,
        rule: &str,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self
            .semantic_workspace()
            .diagnostics_with_document(uri, semantic)
            .into_iter()
            .filter(|diagnostic| diagnostic.rule == rule)
            .collect::<Vec<_>>();
        if let Some(file) = self.files.get(uri) {
            file.lint.retain_unsuppressed(&mut diagnostics);
        }
        lint_diagnostics(source, diagnostics)
    }

    fn indexed_semantic_diagnostics(
        &self,
        uri: &Uri,
        file: &IndexedFile,
        workspace: &sqfmt_lint::SemanticWorkspace<'_, Uri>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = workspace.diagnostics(uri);
        file.lint.retain_unsuppressed(&mut diagnostics);
        let mut diagnostics = lint_diagnostics(&file.source, diagnostics);
        for diagnostic in &mut diagnostics {
            if diagnostic.code
                != Some(NumberOrString::String(
                    sqfmt_lint::DUPLICATE_DECLARATION_RULE.to_string(),
                ))
            {
                continue;
            }
            if let Some(duplicate) = file.semantic.duplicates.iter().find(|duplicate| {
                lsp_range(&file.source, duplicate.range.clone()) == diagnostic.range
            }) {
                diagnostic.related_information = Some(vec![DiagnosticRelatedInformation {
                    location: Location::new(
                        uri.clone(),
                        lsp_range(&file.source, duplicate.previous.clone()),
                    ),
                    message: format!("`{}` is first declared here", duplicate.name),
                }]);
            }
        }
        diagnostics
    }

    pub fn set_folders(&mut self, folders: Vec<WorkspaceFolder>) {
        self.folders = folders;
    }

    pub fn set_api_source_roots(&mut self, mut roots: Vec<PathBuf>) {
        roots.retain(|root| root.is_absolute());
        roots.sort();
        roots.dedup();
        self.api_source_roots = roots;
    }

    pub fn add_folders(&mut self, folders: Vec<WorkspaceFolder>) {
        for folder in folders {
            if !self
                .folders
                .iter()
                .any(|existing| existing.uri == folder.uri)
            {
                self.folders.push(folder);
            }
        }
    }

    pub fn remove_folders(&mut self, folders: &[WorkspaceFolder]) {
        let removed: Vec<_> = folders
            .iter()
            .filter_map(|folder| folder.uri.to_file_path())
            .collect();
        self.folders
            .retain(|folder| !folders.iter().any(|removed| removed.uri == folder.uri));
        self.files.retain(|uri, _| {
            uri.to_file_path()
                .is_none_or(|path| !removed.iter().any(|root| path.starts_with(root.as_ref())))
        });
        self.scripts.retain(|uri, _| {
            uri.to_file_path()
                .is_none_or(|path| !removed.iter().any(|root| path.starts_with(root.as_ref())))
        });
        self.manifests.retain(|uri, _| {
            uri.to_file_path()
                .is_none_or(|path| !removed.iter().any(|root| path.starts_with(root.as_ref())))
        });
        self.api_manifests
            .retain(|uri| self.manifests.contains_key(uri));
    }

    pub fn insert(&mut self, uri: Uri, file: IndexedFile) {
        if self.is_api_uri(&uri) {
            self.api_files.insert(uri.clone());
        } else {
            self.api_files.remove(&uri);
        }
        self.files.insert(uri, file);
    }

    pub fn remove(&mut self, uri: &Uri) {
        self.api_files.remove(uri);
        self.files.remove(uri);
    }

    pub fn is_api_file(&self, uri: &Uri) -> bool {
        self.api_files.contains(uri)
    }

    /// Lint findings for every indexed file, including files no editor currently has open.
    pub fn lint_diagnostic_publications(
        &self,
        advisory_lints: bool,
        open_files: &HashSet<Uri>,
    ) -> Vec<(Uri, Vec<Diagnostic>)> {
        let workspace = sqfmt_lint::Workspace::new(self.files.values().map(|file| &file.lint));
        let semantic_workspace = self.semantic_workspace();
        let mut publications = self
            .files
            .iter()
            .filter(|(uri, _)| !self.api_files.contains(*uri))
            .filter_map(|(uri, file)| {
                let mut diagnostics = lint_diagnostics_for(file, &workspace, advisory_lints);
                if !open_files.contains(uri) {
                    diagnostics.extend(self.indexed_semantic_diagnostics(
                        uri,
                        file,
                        &semantic_workspace,
                    ));
                }
                (!diagnostics.is_empty()).then(|| (uri.clone(), diagnostics))
            })
            .collect::<Vec<_>>();
        publications.extend(
            self.manifests
                .iter()
                .filter(|(uri, _)| !self.api_manifests.contains(*uri))
                .filter_map(|(uri, source)| {
                    let diagnostics =
                        lint_diagnostics(source, workspace.manifest_diagnostics(source));
                    (!diagnostics.is_empty()).then(|| (uri.clone(), diagnostics))
                }),
        );
        publications.sort_by(|left, right| left.0.cmp(&right.0));
        publications
    }

    /// The VMs a file can run in according to the manifest that lists it. Files no manifest
    /// mentions keep every VM open.
    pub fn file_targets(&self, uri: &Uri) -> VmTargets {
        self.scripts
            .get(uri)
            .map_or(VmTargets::ALL, |script| script.targets)
    }

    /// Where a file sits in the project's load sequence, if a manifest lists it.
    pub fn load_order(&self, uri: &Uri) -> Option<LoadOrder> {
        self.scripts.get(uri).map(|script| script.load_order)
    }

    /// The targets of a declaration, narrowed by the VMs its file runs in.
    fn declaration_targets(&self, uri: &Uri, declaration: &OwnedDeclaration) -> VmTargets {
        declaration.targets.intersection(self.file_targets(uri))
    }

    pub fn scan(&mut self) -> usize {
        let workspace_roots: Vec<_> = self
            .folders
            .iter()
            .filter_map(|folder| folder.uri.to_file_path().map(|path| path.into_owned()))
            .collect();
        let mut roots = workspace_roots.clone();
        roots.extend(self.api_source_roots.iter().cloned());
        self.files.retain(|uri, _| {
            uri.to_file_path().is_none_or(|path| {
                !roots.iter().any(|root| path.starts_with(root))
                    || path.is_file() && is_squirrel_file(&path)
            })
        });
        self.api_files.clear();
        self.api_manifests.clear();
        self.scripts.clear();
        self.manifests.clear();
        let mut workspace_manifests = Vec::new();
        for root in workspace_roots {
            self.scan_directory(&root, &mut workspace_manifests);
        }
        let mut api_manifests = Vec::new();
        for root in self.api_source_roots.clone() {
            self.scan_directory(&root, &mut api_manifests);
        }
        self.load_manifests(workspace_manifests, false);
        self.load_manifests(api_manifests, true);
        self.files.len()
    }

    /// Rereads every manifest, for when one changed on disk.
    pub fn rescan_manifests(&mut self) {
        let mut workspace_manifests = Vec::new();
        for root in self
            .folders
            .iter()
            .filter_map(|folder| folder.uri.to_file_path().map(|path| path.into_owned()))
            .collect::<Vec<_>>()
        {
            collect_manifests(&root, &mut workspace_manifests);
        }
        let mut api_manifests = Vec::new();
        for root in &self.api_source_roots {
            collect_manifests(root, &mut api_manifests);
        }
        self.scripts.clear();
        self.manifests.clear();
        self.api_manifests.clear();
        self.load_manifests(workspace_manifests, false);
        self.load_manifests(api_manifests, true);
    }

    fn load_manifests(&mut self, manifests: Vec<PathBuf>, api: bool) {
        for manifest in manifests {
            if let Some(uri) = Uri::from_file_path(&manifest)
                && let Ok(source) = std::fs::read_to_string(&manifest)
            {
                if api && self.is_api_uri(&uri) {
                    self.api_manifests.insert(uri.clone());
                }
                self.manifests.insert(uri, source);
            }
            for (path, entry) in read_manifest(&manifest) {
                if let Some(uri) = Uri::from_file_path(&path) {
                    self.scripts.insert(uri, entry);
                }
            }
        }
    }

    pub fn query(&self, query: &str, limit: usize) -> Vec<WorkspaceSymbol> {
        let mut matches = self
            .files
            .iter()
            .flat_map(|(uri, file)| {
                workspace_symbols(&file.symbols, query)
                    .into_iter()
                    .map(move |symbol| {
                        let range = tower_lsp_server::ls_types::Range::new(
                            position_at(&file.source, symbol.selection_range.start),
                            position_at(&file.source, symbol.selection_range.end),
                        );
                        (
                            symbol.score,
                            WorkspaceSymbol {
                                name: symbol.name,
                                kind: symbol.kind,
                                tags: None,
                                container_name: symbol.container_name,
                                location: OneOf::Left(Location::new(uri.clone(), range)),
                                data: None,
                            },
                        )
                    })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_score, left), (right_score, right)| {
            left_score
                .cmp(right_score)
                .then_with(|| {
                    left.name
                        .to_ascii_lowercase()
                        .cmp(&right.name.to_ascii_lowercase())
                })
                .then_with(|| left.location_uri().cmp(right.location_uri()))
        });
        matches
            .into_iter()
            .take(limit)
            .map(|(_, symbol)| symbol)
            .collect()
    }

    /// Definitions of an exported name, keeping only those a caller in `targets` could reach.
    pub fn definitions(&self, name: &str, targets: VmTargets) -> Vec<Location> {
        let mut locations = self
            .files
            .iter()
            .flat_map(|(uri, file)| {
                file.semantic
                    .declarations
                    .iter()
                    .filter(move |declaration| {
                        declaration.is_global
                            && declaration.name == name
                            && self
                                .declaration_targets(uri, declaration)
                                .compatible_with(targets)
                    })
                    .map(move |declaration| {
                        Location::new(
                            uri.clone(),
                            tower_lsp_server::ls_types::Range::new(
                                position_at(&file.source, declaration.range.start),
                                position_at(&file.source, declaration.range.end),
                            ),
                        )
                    })
            })
            .collect::<Vec<_>>();
        // Definitions are presented in the order the project loads them, so a duplicated global
        // reads in the sequence the game would see it.
        locations.sort_by(|left, right| {
            self.load_order(&left.uri)
                .cmp(&self.load_order(&right.uri))
                .then_with(|| left.uri.cmp(&right.uri))
                .then_with(|| left.range.start.cmp(&right.range.start))
        });
        locations
    }

    pub fn global_occurrences(&self, name: &str) -> Vec<WorkspaceOccurrence> {
        let mut occurrences = self
            .files
            .iter()
            .flat_map(|(uri, file)| {
                let declarations = file
                    .semantic
                    .declarations
                    .iter()
                    .filter(move |declaration| declaration.is_global && declaration.name == name)
                    .map(move |declaration| WorkspaceOccurrence {
                        uri: uri.clone(),
                        range: lsp_range(&file.source, declaration.range.clone()),
                        declaration: true,
                    });
                let references =
                    file.semantic
                        .global_references(name)
                        .into_iter()
                        .map(move |range| WorkspaceOccurrence {
                            uri: uri.clone(),
                            range: lsp_range(&file.source, range),
                            declaration: false,
                        });
                declarations.chain(references)
            })
            .collect::<Vec<_>>();
        occurrences.sort_by(|left, right| {
            left.uri
                .cmp(&right.uri)
                .then_with(|| left.range.start.cmp(&right.range.start))
                .then_with(|| right.declaration.cmp(&left.declaration))
        });
        occurrences
    }

    pub fn global_declarations(&self, name: &str) -> Vec<WorkspaceDeclaration> {
        let mut declarations = self
            .files
            .iter()
            .flat_map(|(uri, file)| {
                file.semantic
                    .declarations
                    .iter()
                    .filter(move |declaration| declaration.is_global && declaration.name == name)
                    .map(move |declaration| WorkspaceDeclaration {
                        uri: uri.clone(),
                        range: lsp_range(&file.source, declaration.range.clone()),
                        name: declaration.name.clone(),
                        kind: declaration.kind,
                        detail: declaration.detail.clone(),
                        targets: self.declaration_targets(uri, declaration),
                    })
            })
            .collect::<Vec<_>>();
        declarations.sort_by(|left, right| {
            left.uri
                .cmp(&right.uri)
                .then_with(|| left.range.start.cmp(&right.range.start))
        });
        declarations
    }

    /// The kind an exported name is declared with, when every declaration agrees.
    pub fn global_declaration_kind(&self, name: &str) -> Option<DeclarationKind> {
        unique_value(
            self.files
                .values()
                .flat_map(|file| file.semantic.declarations.iter())
                .filter(|declaration| declaration.is_global && declaration.name == name)
                .map(|declaration| declaration.kind),
        )
    }

    pub fn globals(&self) -> Vec<WorkspaceDeclaration> {
        let mut declarations = self
            .files
            .iter()
            .flat_map(|(uri, file)| {
                file.semantic
                    .declarations
                    .iter()
                    .filter(|declaration| declaration.is_global)
                    .map(move |declaration| WorkspaceDeclaration {
                        uri: uri.clone(),
                        range: lsp_range(&file.source, declaration.range.clone()),
                        name: declaration.name.clone(),
                        kind: declaration.kind,
                        detail: declaration.detail.clone(),
                        targets: self.declaration_targets(uri, declaration),
                    })
            })
            .collect::<Vec<_>>();
        declarations.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.uri.cmp(&right.uri))
                .then_with(|| left.range.start.cmp(&right.range.start))
        });
        declarations
    }

    pub fn resolve_value_owner(&self, uri: &Uri, value: &ValueSource) -> Option<ResolvedType> {
        self.semantic_workspace().resolve_value_owner(uri, value)
    }

    pub fn resolve_value_owner_with_document(
        &self,
        uri: &Uri,
        semantic: &SemanticDocument,
        value: &ValueSource,
    ) -> Option<ResolvedType> {
        self.semantic_workspace()
            .resolve_value_owner_with_document(uri, semantic, value)
    }

    pub fn resolve_type_identity(
        &self,
        uri: &Uri,
        identity: &TypeIdentity,
    ) -> Option<ResolvedType> {
        self.semantic_workspace()
            .resolve_type_identity(uri, identity)
    }

    pub fn callable_signatures_with_document(
        &self,
        uri: &Uri,
        semantic: &SemanticDocument,
        callable: &ValueSource,
    ) -> Vec<OwnedSignature> {
        self.semantic_workspace()
            .callable_signatures_with_document(uri, semantic, callable)
    }

    pub fn members_for_type_at_with_document(
        &self,
        owner: &ResolvedType,
        override_uri: Option<&Uri>,
        override_semantic: Option<&SemanticDocument>,
        offset: usize,
    ) -> Vec<WorkspaceMember> {
        self.semantic_workspace()
            .members_for_type_at_with_document(owner, override_uri, override_semantic, offset)
            .into_iter()
            .filter_map(|member| self.workspace_member(member))
            .collect()
    }

    fn workspace_member(&self, member: sqfmt_lint::SemanticMember<Uri>) -> Option<WorkspaceMember> {
        let source = &self.files.get(&member.file)?.source;
        Some(WorkspaceMember {
            uri: member.file,
            range: lsp_range(source, member.range),
            name: member.name,
            kind: member.kind,
            detail: member.detail,
            targets: member.targets,
        })
    }

    pub fn member_declarations_for_type(
        &self,
        owner: &ResolvedType,
        name: &str,
    ) -> Vec<WorkspaceMember> {
        self.semantic_workspace()
            .member_declarations_for_type(owner, name)
            .into_iter()
            .filter_map(|member| self.workspace_member(member))
            .collect()
    }

    /// Member accesses whose owner is a fully known type that declares no such member.
    ///
    /// Absence only proves anything when the member list is complete, so this reports a name only
    /// when every link of the owner's chain is a `struct` or `class` declared in the workspace.
    /// Native types such as `entity`, tables, and per-instance slots stay silent.
    pub fn invalid_member_diagnostics(
        &self,
        uri: &Uri,
        source: &str,
        semantic: &SemanticDocument,
    ) -> Vec<Diagnostic> {
        self.semantic_rule_diagnostics(uri, source, semantic, sqfmt_lint::INVALID_MEMBER_RULE)
    }

    /// Calls that pass a number of arguments no declaration of their callee accepts.
    ///
    /// Squirrel rejects a wrong argument count at run time, so this is a real defect, but only when
    /// the parameter list is known exactly. A call whose callee resolves to nothing, to a value with
    /// no signature, or to a class that may inherit a constructor from outside the workspace is left
    /// alone, which is most calls in a mod repository.
    pub fn call_arity_diagnostics(
        &self,
        uri: &Uri,
        source: &str,
        semantic: &SemanticDocument,
    ) -> Vec<Diagnostic> {
        self.semantic_rule_diagnostics(uri, source, semantic, sqfmt_lint::CALL_ARITY_RULE)
    }

    /// Arguments whose known nominal type no viable declaration of the callee accepts.
    pub fn call_argument_type_diagnostics(
        &self,
        uri: &Uri,
        source: &str,
        semantic: &SemanticDocument,
    ) -> Vec<Diagnostic> {
        self.semantic_rule_diagnostics(uri, source, semantic, sqfmt_lint::ARGUMENT_TYPE_RULE)
    }

    /// Initializers and `return` values whose type contradicts a declared one.
    ///
    /// Only nominal types both sides fully declare are compared. A declared type resolves through
    /// typedef aliases, and a value satisfies it when the declared name appears anywhere in the
    /// value's own base chain, so passing a subclass where a base is declared stays silent.
    pub fn type_mismatch_diagnostics(
        &self,
        uri: &Uri,
        source: &str,
        semantic: &SemanticDocument,
    ) -> Vec<Diagnostic> {
        let rules = [
            sqfmt_lint::INITIALIZER_TYPE_RULE,
            sqfmt_lint::RETURN_TYPE_RULE,
        ];
        let mut diagnostics = self
            .semantic_workspace()
            .diagnostics_with_document(uri, semantic)
            .into_iter()
            .filter(|diagnostic| rules.contains(&diagnostic.rule))
            .collect::<Vec<_>>();
        if let Some(file) = self.files.get(uri) {
            file.lint.retain_unsuppressed(&mut diagnostics);
        }
        lint_diagnostics(source, diagnostics)
    }

    pub fn member_owner_for_type(&self, owner: &ResolvedType, name: &str) -> Option<ResolvedType> {
        self.semantic_workspace().member_owner_for_type(owner, name)
    }

    pub fn member_occurrences_for_type(
        &self,
        owner: &ResolvedType,
        name: &str,
    ) -> Vec<WorkspaceOccurrence> {
        let mut occurrences = self
            .files
            .iter()
            .flat_map(|(uri, file)| {
                let declarations = file
                    .semantic
                    .declarations
                    .iter()
                    .filter(move |declaration| {
                        declaration_owner_matches(uri, declaration, owner)
                            && declaration.name == name
                    })
                    .map(move |declaration| WorkspaceOccurrence {
                        uri: uri.clone(),
                        range: lsp_range(&file.source, declaration.range.clone()),
                        declaration: true,
                    });
                let references = file
                    .semantic
                    .member_references
                    .iter()
                    .filter(move |reference| {
                        if !reference.available || reference.name != name {
                            return false;
                        }
                        self.resolve_value_owner(uri, &reference.receiver)
                            .and_then(|receiver| self.member_owner_for_type(&receiver, name))
                            .as_ref()
                            == Some(owner)
                    })
                    .map(move |reference| WorkspaceOccurrence {
                        uri: uri.clone(),
                        range: lsp_range(&file.source, reference.range.clone()),
                        declaration: false,
                    });
                let bare_references = file
                    .semantic
                    .references
                    .iter()
                    .filter(move |reference| {
                        reference.name == name
                            && reference.target.as_ref().is_some_and(|target| {
                                file.semantic.declaration_for_range(target).is_some_and(
                                    |declaration| {
                                        declaration_owner_matches(uri, declaration, owner)
                                    },
                                )
                            })
                    })
                    .map(move |reference| WorkspaceOccurrence {
                        uri: uri.clone(),
                        range: lsp_range(&file.source, reference.range.clone()),
                        declaration: false,
                    });
                declarations.chain(references).chain(bare_references)
            })
            .collect::<Vec<_>>();
        occurrences.sort_by(|left, right| {
            left.uri
                .cmp(&right.uri)
                .then_with(|| left.range.start.cmp(&right.range.start))
                .then_with(|| right.declaration.cmp(&left.declaration))
        });
        occurrences
    }

    fn scan_directory(&mut self, directory: &Path, manifests: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                self.scan_directory(&path, manifests);
            } else if path.file_name().is_some_and(|name| name == "mod.json") {
                manifests.push(path);
            } else if is_squirrel_file(&path)
                && let Some(uri) = Uri::from_file_path(&path)
                && let Some(indexed) = IndexedFile::read(&uri)
            {
                self.insert(uri, indexed);
            }
        }
    }

    fn is_api_uri(&self, uri: &Uri) -> bool {
        let Some(path) = uri.to_file_path() else {
            return false;
        };
        self.api_source_roots
            .iter()
            .any(|root| path.starts_with(root))
            && !self.folders.iter().any(|folder| {
                folder
                    .uri
                    .to_file_path()
                    .is_some_and(|root| path.starts_with(root.as_ref()))
            })
    }
}

fn collect_manifests(directory: &Path, manifests: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_manifests(&path, manifests);
        } else if path.file_name().is_some_and(|name| name == "mod.json") {
            manifests.push(path);
        }
    }
}

trait WorkspaceSymbolExt {
    fn location_uri(&self) -> &Uri;
}

impl WorkspaceSymbolExt for WorkspaceSymbol {
    fn location_uri(&self) -> &Uri {
        match &self.location {
            OneOf::Left(location) => &location.uri,
            OneOf::Right(location) => &location.uri,
        }
    }
}

fn is_squirrel_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "nut" | "gnut"))
}

fn unique_value<T: Eq>(values: impl Iterator<Item = T>) -> Option<T> {
    let mut values = values;
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

fn declaration_owner_matches(
    uri: &Uri,
    declaration: &OwnedDeclaration,
    owner: &ResolvedType,
) -> bool {
    match (declaration.owner.as_ref(), owner) {
        (Some(TypeIdentity::Nominal(declaration_owner)), ResolvedType::Nominal(owner)) => {
            declaration_owner == owner
        }
        (
            Some(TypeIdentity::Structural(declaration_owner)),
            ResolvedType::Structural {
                file: owner_uri,
                range,
            },
        ) => uri == owner_uri && declaration_owner == range,
        _ => false,
    }
}

fn lint_diagnostics_for(
    file: &IndexedFile,
    workspace: &sqfmt_lint::Workspace,
    advisory_lints: bool,
) -> Vec<Diagnostic> {
    lint_diagnostics(
        &file.source,
        workspace.diagnostics_with_options(
            &file.lint,
            sqfmt_lint::LintOptions {
                advisory: advisory_lints,
            },
        ),
    )
}

fn lint_diagnostics(source: &str, diagnostics: Vec<sqfmt_lint::Diagnostic>) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|lint| {
            let mut diagnostic = sqformat_lsp::warning(source, lint.range, lint.message);
            diagnostic.code = Some(NumberOrString::String(lint.rule.to_string()));
            diagnostic
        })
        .collect()
}

fn lsp_range(source: &str, range: std::ops::Range<usize>) -> tower_lsp_server::ls_types::Range {
    tower_lsp_server::ls_types::Range::new(
        position_at(source, range.start),
        position_at(source, range.end),
    )
}
