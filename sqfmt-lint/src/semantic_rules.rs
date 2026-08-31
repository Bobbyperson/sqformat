use std::collections::HashSet;
use std::hash::Hash;
use std::ops::Range;

use crate::{
    DeclarationKind, Diagnostic, OwnedDeclaration, OwnedSignature, SemanticDocument, TypeIdentity,
    ValueSource, VmTargets,
};

pub const DUPLICATE_DECLARATION_RULE: &str = "duplicate-declaration";
pub const INVALID_MEMBER_RULE: &str = "invalid-member";
pub const CALL_ARITY_RULE: &str = "call-arity";
pub const ARGUMENT_TYPE_RULE: &str = "argument-type";
pub const INITIALIZER_TYPE_RULE: &str = "initializer-type";
pub const RETURN_TYPE_RULE: &str = "return-type";

#[derive(Clone, Debug)]
pub struct SemanticFile<'a, I> {
    pub id: I,
    pub document: &'a SemanticDocument,
    pub targets: VmTargets,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ResolvedType<I> {
    Nominal(String),
    Structural {
        file: I,
        range: Range<usize>,
    },
    Augmented {
        base: Box<ResolvedType<I>>,
        file: I,
        range: Range<usize>,
    },
}

#[derive(Clone, Debug)]
pub struct SemanticMember<I> {
    pub file: I,
    pub range: Range<usize>,
    pub name: String,
    pub kind: DeclarationKind,
    pub detail: String,
    pub targets: VmTargets,
}

pub struct SemanticWorkspace<'a, I> {
    files: Vec<SemanticFile<'a, I>>,
}

impl<'a, I> SemanticWorkspace<'a, I>
where
    I: Clone + Eq + Hash + Ord,
{
    pub fn new(files: impl IntoIterator<Item = SemanticFile<'a, I>>) -> Self {
        Self {
            files: files.into_iter().collect(),
        }
    }

    pub fn diagnostics(&self, file: &I) -> Vec<Diagnostic> {
        let Some(document) = self.document(file) else {
            return Vec::new();
        };
        self.diagnostics_with_document(file, document)
    }

    pub fn diagnostics_with_document(
        &self,
        file: &I,
        document: &SemanticDocument,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self.duplicate_diagnostics(document);
        diagnostics.extend(self.invalid_member_diagnostics(file, document));
        diagnostics.extend(self.call_arity_diagnostics(file, document));
        diagnostics.extend(self.argument_type_diagnostics(file, document));
        diagnostics.extend(self.type_diagnostics(file, document));
        diagnostics.sort_by(|left, right| {
            left.range
                .start
                .cmp(&right.range.start)
                .then_with(|| left.range.end.cmp(&right.range.end))
                .then_with(|| left.rule.cmp(right.rule))
        });
        diagnostics
    }

    fn duplicate_diagnostics(&self, document: &SemanticDocument) -> Vec<Diagnostic> {
        document
            .duplicates
            .iter()
            .map(|duplicate| {
                let shadows_parameter = duplicate.previous_kind == DeclarationKind::Parameter
                    && duplicate.kind != DeclarationKind::Parameter;
                let message = if shadows_parameter {
                    format!(
                        "`{}` shadows the parameter of the same name",
                        duplicate.name
                    )
                } else {
                    format!("`{}` is already declared in this scope", duplicate.name)
                };
                diagnostic(duplicate.range.clone(), DUPLICATE_DECLARATION_RULE, message)
            })
            .collect()
    }

    fn invalid_member_diagnostics(&self, file: &I, document: &SemanticDocument) -> Vec<Diagnostic> {
        document
            .member_references
            .iter()
            .filter(|reference| reference.available)
            .filter_map(|reference| {
                let owner =
                    self.resolve_value_owner_with_document(file, document, &reference.receiver)?;
                let chain = self.closed_owner_chain(&owner, Some(file), Some(document))?;
                let declared = chain.iter().any(|owner| {
                    self.direct_members(
                        &ResolvedType::Nominal(owner.clone()),
                        Some(file),
                        Some(document),
                        None,
                    )
                    .iter()
                    .any(|member| member.name == reference.name)
                });
                (!declared).then(|| {
                    diagnostic(
                        reference.range.clone(),
                        INVALID_MEMBER_RULE,
                        format!("`{}` is not a member of `{}`", reference.name, chain[0]),
                    )
                })
            })
            .collect()
    }

    fn call_arity_diagnostics(&self, file: &I, document: &SemanticDocument) -> Vec<Diagnostic> {
        document
            .calls
            .iter()
            .filter_map(|call| {
                let signatures = self.arity_signatures(file, document, &call.callable)?;
                (!signatures
                    .iter()
                    .any(|signature| accepts_arguments(signature, call.arguments.len())))
                .then(|| {
                    diagnostic(
                        call.open.start..call.close.end,
                        CALL_ARITY_RULE,
                        arity_message(&signatures, call.arguments.len()),
                    )
                })
            })
            .collect()
    }

    fn argument_type_diagnostics(&self, file: &I, document: &SemanticDocument) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for call in &document.calls {
            let Some(signatures) = self.arity_signatures(file, document, &call.callable) else {
                continue;
            };
            let viable = signatures
                .iter()
                .filter(|signature| accepts_arguments(signature, call.arguments.len()))
                .collect::<Vec<_>>();
            for (index, argument) in call.arguments.iter().enumerate() {
                let mut rejected = Vec::new();
                let mut inconclusive = viable.is_empty();
                for signature in &viable {
                    let Some(parameter) = signature.parameters.get(index) else {
                        inconclusive = true;
                        break;
                    };
                    if parameter.variadic {
                        inconclusive = true;
                        break;
                    }
                    let Some(expected) = &parameter.type_identity else {
                        inconclusive = true;
                        break;
                    };
                    let Some((actual, expected, compatible)) =
                        self.type_comparison(file, document, expected, &argument.source)
                    else {
                        inconclusive = true;
                        break;
                    };
                    if compatible {
                        inconclusive = true;
                        break;
                    }
                    rejected.push((actual, expected));
                }
                if inconclusive || rejected.len() != viable.len() {
                    continue;
                }
                let (actual, expected) = &rejected[0];
                let message = if rejected.iter().all(|(_, candidate)| candidate == expected) {
                    format!("`{actual}` is not a `{expected}`")
                } else {
                    format!(
                        "`{actual}` is not accepted for argument {} by any declaration of this call",
                        index + 1
                    )
                };
                diagnostics.push(diagnostic(
                    argument.range.clone(),
                    ARGUMENT_TYPE_RULE,
                    message,
                ));
            }
        }
        diagnostics
    }

    fn type_diagnostics(&self, file: &I, document: &SemanticDocument) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for declaration in &document.declarations {
            if let Some(identity) = &declaration.type_identity
                && let Some(initializer) = &declaration.initializer_source
                && let Some(message) = self.mismatch(file, document, identity, initializer)
            {
                diagnostics.push(diagnostic(
                    declaration.range.clone(),
                    INITIALIZER_TYPE_RULE,
                    message,
                ));
            }
            let Some(declared) = &declaration.return_type else {
                continue;
            };
            let identity = TypeIdentity::Nominal(declared.clone());
            for returned in &declaration.returns {
                if let Some(message) = self.mismatch(file, document, &identity, &returned.source) {
                    diagnostics.push(diagnostic(
                        returned.range.clone(),
                        RETURN_TYPE_RULE,
                        message,
                    ));
                }
            }
        }
        diagnostics
    }

    fn mismatch(
        &self,
        file: &I,
        document: &SemanticDocument,
        declared: &TypeIdentity,
        value: &ValueSource,
    ) -> Option<String> {
        let (actual, declared, compatible) =
            self.type_comparison(file, document, declared, value)?;
        (!compatible).then(|| format!("`{actual}` is not a `{declared}`"))
    }

    fn type_comparison(
        &self,
        file: &I,
        document: &SemanticDocument,
        declared: &TypeIdentity,
        value: &ValueSource,
    ) -> Option<(String, String, bool)> {
        let declared =
            self.resolve_type_identity_with_override(file, declared, Some(file), Some(document))?;
        let declared = self.closed_owner_chain(&declared, Some(file), Some(document))?;
        let declared = declared.first()?.clone();
        let actual = self.resolve_value_owner_with_document(file, document, value)?;
        let actual = self.closed_owner_chain(&actual, Some(file), Some(document))?;
        Some((
            actual[0].clone(),
            declared.clone(),
            actual.contains(&declared),
        ))
    }

    pub fn resolve_value_owner(&self, file: &I, value: &ValueSource) -> Option<ResolvedType<I>> {
        let document = self.document(file)?;
        self.resolve_value_owner_in(Some(file), document, value, Some(file), None, 0)
    }

    pub fn resolve_value_owner_with_document(
        &self,
        file: &I,
        document: &SemanticDocument,
        value: &ValueSource,
    ) -> Option<ResolvedType<I>> {
        self.resolve_value_owner_in(Some(file), document, value, Some(file), Some(document), 0)
    }

    pub fn resolve_type_identity(
        &self,
        file: &I,
        identity: &TypeIdentity,
    ) -> Option<ResolvedType<I>> {
        self.resolve_type_identity_with_override(file, identity, Some(file), None)
    }

    pub fn callable_signatures_with_document(
        &self,
        file: &I,
        document: &SemanticDocument,
        callable: &ValueSource,
    ) -> Vec<OwnedSignature> {
        let mut signatures: Vec<OwnedSignature> = match callable {
            ValueSource::Callable(range) => {
                document.callables.get(range).cloned().into_iter().collect()
            }
            ValueSource::Call(callee) => self
                .callee_declarations(file, document, callee)
                .into_iter()
                .filter_map(|(_, declaration)| declaration.return_signature)
                .collect(),
            _ => self
                .callee_declarations(file, document, callable)
                .into_iter()
                .flat_map(|(_, declaration)| {
                    self.callable_declaration_signatures(&declaration, Some(file), Some(document))
                })
                .collect(),
        };
        signatures.sort_by(|left, right| left.label.cmp(&right.label));
        signatures.dedup();
        signatures
    }

    pub fn members_for_type_at_with_document(
        &self,
        owner: &ResolvedType<I>,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
        offset: usize,
    ) -> Vec<SemanticMember<I>> {
        self.members_for_type_with_options(owner, override_file, override_document, Some(offset))
    }

    pub fn members_for_type_with_document(
        &self,
        owner: &ResolvedType<I>,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
    ) -> Vec<SemanticMember<I>> {
        self.members_for_type_with_options(owner, override_file, override_document, None)
    }

    pub fn member_owner_for_type(
        &self,
        owner: &ResolvedType<I>,
        name: &str,
    ) -> Option<ResolvedType<I>> {
        self.resolved_owner_chain(owner, None, None)
            .into_iter()
            .find(|owner| {
                self.direct_members(owner, None, None, None)
                    .iter()
                    .any(|member| member.name == name)
            })
    }

    pub fn member_declarations_for_type(
        &self,
        owner: &ResolvedType<I>,
        name: &str,
    ) -> Vec<SemanticMember<I>> {
        let Some(owner) = self.member_owner_for_type(owner, name) else {
            return Vec::new();
        };
        self.direct_members(&owner, None, None, None)
            .into_iter()
            .filter(|member| member.name == name)
            .collect()
    }

    fn members_for_type_with_options(
        &self,
        owner: &ResolvedType<I>,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
        offset: Option<usize>,
    ) -> Vec<SemanticMember<I>> {
        let mut names = HashSet::new();
        let mut result = Vec::new();
        for owner in self.resolved_owner_chain(owner, override_file, override_document) {
            for member in self.direct_members(&owner, override_file, override_document, offset) {
                if names.insert(member.name.clone()) {
                    result.push(member);
                }
            }
        }
        result.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.file.cmp(&right.file))
                .then_with(|| left.range.start.cmp(&right.range.start))
        });
        result
    }

    fn direct_members(
        &self,
        owner: &ResolvedType<I>,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
        offset: Option<usize>,
    ) -> Vec<SemanticMember<I>> {
        self.files
            .iter()
            .filter(|file| {
                !matches!(owner, ResolvedType::Structural { file: owner_file, .. } if owner_file != &file.id)
            })
            .flat_map(|file| {
                let document = self.document_with_override(
                    &file.id,
                    override_file,
                    override_document,
                );
                owner_identity(&file.id, owner)
                    .map(|identity| document.declarations_owned_by(&identity).collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(move |declaration| {
                        !matches!(owner, ResolvedType::Structural { .. })
                            || offset.is_none_or(|offset| {
                                declaration
                                    .available_from
                                    .unwrap_or(declaration.range.start)
                                    <= offset
                            })
                    })
                    .map(move |declaration| SemanticMember {
                        file: file.id.clone(),
                        range: declaration.range.clone(),
                        name: declaration.name.clone(),
                        kind: declaration.kind,
                        detail: declaration.detail.clone(),
                        targets: declaration.targets.intersection(file.targets),
                    })
            })
            .collect()
    }

    fn resolve_value_owner_in(
        &self,
        file: Option<&I>,
        document: &SemanticDocument,
        value: &ValueSource,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
        depth: usize,
    ) -> Option<ResolvedType<I>> {
        if depth >= 32 {
            return None;
        }
        match value {
            ValueSource::Unknown | ValueSource::Callable(_) => None,
            ValueSource::Augmented { base, extra } => {
                let file = file?.clone();
                match self.resolve_value_owner_in(
                    Some(&file),
                    document,
                    base,
                    override_file,
                    override_document,
                    depth + 1,
                ) {
                    Some(base) => Some(ResolvedType::Augmented {
                        base: Box::new(base),
                        file,
                        range: extra.clone(),
                    }),
                    None => Some(ResolvedType::Structural {
                        file,
                        range: extra.clone(),
                    }),
                }
            }
            ValueSource::Type(identity) => self.resolve_type_identity_with_override(
                file?,
                identity,
                override_file,
                override_document,
            ),
            ValueSource::Declaration(range) => {
                document
                    .declaration_for_range(range)
                    .and_then(|declaration| {
                        self.declaration_value_type(
                            file,
                            declaration,
                            false,
                            override_file,
                            override_document,
                            depth,
                        )
                    })
            }
            ValueSource::Workspace(name) => unique_value(
                self.named_declarations(name, override_file, override_document, false)
                    .iter()
                    .filter_map(|(file, declaration)| {
                        self.declaration_value_type(
                            Some(file),
                            declaration,
                            false,
                            override_file,
                            override_document,
                            depth,
                        )
                    }),
            ),
            ValueSource::Call(function) => {
                let declarations = self.callee_declarations_for_source(
                    file?,
                    document,
                    function,
                    override_file,
                    override_document,
                );
                unique_value(declarations.iter().filter_map(|(file, declaration)| {
                    self.declaration_value_type(
                        Some(file),
                        declaration,
                        true,
                        override_file,
                        override_document,
                        depth,
                    )
                }))
            }
            ValueSource::Member { receiver, name } => {
                let owner = self.resolve_value_owner_in(
                    file,
                    document,
                    receiver,
                    override_file,
                    override_document,
                    depth + 1,
                )?;
                unique_value(
                    self.member_declaration_values_for_type(
                        &owner,
                        name,
                        override_file,
                        override_document,
                    )
                    .iter()
                    .filter_map(|(file, declaration)| {
                        self.declaration_value_type(
                            Some(file),
                            declaration,
                            false,
                            override_file,
                            override_document,
                            depth,
                        )
                    }),
                )
            }
        }
    }

    fn declaration_value_type(
        &self,
        declaring_file: Option<&I>,
        declaration: &OwnedDeclaration,
        called: bool,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
        depth: usize,
    ) -> Option<ResolvedType<I>> {
        if !called && let Some(identity) = &declaration.type_identity {
            return self.resolve_type_identity_with_override(
                declaring_file?,
                identity,
                override_file,
                override_document,
            );
        }
        let document = || {
            declaring_file
                .map(|file| self.document_with_override(file, override_file, override_document))
        };
        let type_ = if called {
            let declared = declaration
                .type_object
                .as_ref()
                .or(declaration.return_type.as_ref());
            if declared.is_none() {
                let document = document()?;
                let mut returned = declaration.returns.iter().map(|returned| {
                    self.resolve_value_owner_in(
                        declaring_file,
                        document,
                        &returned.source,
                        override_file,
                        override_document,
                        depth + 1,
                    )
                });
                let first = returned.next()??;
                return returned
                    .all(|type_| type_.as_ref() == Some(&first))
                    .then_some(first);
            }
            declared
        } else {
            let nominal = declaration
                .declared_type
                .as_ref()
                .filter(|type_| !matches!(type_.as_str(), "local" | "var"))
                .or(declaration.type_object.as_ref());
            if nominal.is_none() {
                let document = document()?;
                return declaration.initializer_source.as_ref().and_then(|source| {
                    self.resolve_value_owner_in(
                        declaring_file,
                        document,
                        source,
                        override_file,
                        override_document,
                        depth + 1,
                    )
                });
            }
            nominal
        }?;
        self.resolve_nominal_type(type_, override_file, override_document, &mut HashSet::new())
            .map(ResolvedType::Nominal)
    }

    fn resolve_type_identity_with_override(
        &self,
        file: &I,
        identity: &TypeIdentity,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
    ) -> Option<ResolvedType<I>> {
        match identity {
            TypeIdentity::Nominal(name) => self
                .resolve_nominal_type(name, override_file, override_document, &mut HashSet::new())
                .map(ResolvedType::Nominal),
            TypeIdentity::Structural(range) => Some(ResolvedType::Structural {
                file: file.clone(),
                range: range.clone(),
            }),
        }
    }

    fn resolve_nominal_type(
        &self,
        name: &str,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
        visited: &mut HashSet<String>,
    ) -> Option<String> {
        if !visited.insert(name.to_string()) {
            return None;
        }
        let declarations = self.named_declarations(name, override_file, override_document, true);
        let aliases = declarations
            .iter()
            .map(|(_, declaration)| declaration)
            .filter(|declaration| declaration.kind == DeclarationKind::Type)
            .collect::<Vec<_>>();
        if aliases.is_empty() {
            return Some(name.to_string());
        }
        if declarations
            .iter()
            .any(|(_, declaration)| declaration.type_object.is_some())
        {
            return None;
        }
        unique_value(aliases.into_iter().filter_map(|declaration| {
            self.resolve_nominal_type(
                declaration.declared_type.as_deref()?,
                override_file,
                override_document,
                &mut visited.clone(),
            )
        }))
    }

    fn resolved_owner_chain(
        &self,
        owner: &ResolvedType<I>,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
    ) -> Vec<ResolvedType<I>> {
        match owner {
            ResolvedType::Nominal(name) => self
                .owner_chain(name, override_file, override_document)
                .map(|owners| owners.into_iter().map(ResolvedType::Nominal).collect())
                .unwrap_or_default(),
            ResolvedType::Structural { .. } => vec![owner.clone()],
            ResolvedType::Augmented { base, file, range } => {
                let mut owners = self.resolved_owner_chain(base, override_file, override_document);
                owners.push(ResolvedType::Structural {
                    file: file.clone(),
                    range: range.clone(),
                });
                owners
            }
        }
    }

    fn owner_chain(
        &self,
        owner: &str,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
    ) -> Result<Vec<String>, ()> {
        let mut chain = Vec::new();
        let mut current = owner.to_string();
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                return Err(());
            }
            chain.push(current.clone());
            let declarations =
                self.named_declarations(&current, override_file, override_document, true);
            let classes = declarations
                .iter()
                .map(|(_, declaration)| declaration)
                .filter(|declaration| declaration.kind == DeclarationKind::Class)
                .collect::<Vec<_>>();
            if classes.is_empty() {
                return Ok(chain);
            }
            let bases = classes
                .iter()
                .map(|declaration| declaration.base_type.as_deref())
                .collect::<Vec<_>>();
            if bases.iter().all(Option::is_none) {
                return Ok(chain);
            }
            let Some(base) = bases.first().copied().flatten() else {
                return Err(());
            };
            if !bases.iter().all(|candidate| *candidate == Some(base)) {
                return Err(());
            }
            current = self
                .resolve_nominal_type(base, override_file, override_document, &mut HashSet::new())
                .ok_or(())?;
        }
    }

    fn closed_owner_chain(
        &self,
        owner: &ResolvedType<I>,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
    ) -> Option<Vec<String>> {
        let ResolvedType::Nominal(name) = owner else {
            return None;
        };
        let chain = self
            .owner_chain(name, override_file, override_document)
            .ok()?;
        chain
            .iter()
            .all(|name| {
                self.named_declarations(name, override_file, override_document, true)
                    .iter()
                    .any(|(_, declaration)| {
                        matches!(
                            declaration.kind,
                            DeclarationKind::Struct | DeclarationKind::Class
                        )
                    })
            })
            .then_some(chain)
    }

    fn arity_signatures(
        &self,
        file: &I,
        document: &SemanticDocument,
        callable: &ValueSource,
    ) -> Option<Vec<OwnedSignature>> {
        let declarations = match callable {
            ValueSource::Callable(range) => {
                return document
                    .callables
                    .get(range)
                    .cloned()
                    .map(|signature| vec![signature]);
            }
            ValueSource::Call(callee) => {
                let declarations = self.callee_declarations(file, document, callee);
                if declarations.is_empty() {
                    return None;
                }
                return declarations
                    .into_iter()
                    .map(|(_, declaration)| declaration.return_signature)
                    .collect();
            }
            ValueSource::Declaration(_)
            | ValueSource::Workspace(_)
            | ValueSource::Member { .. } => self.callee_declarations(file, document, callable),
            _ => return None,
        };
        if declarations.is_empty()
            || !matches!(callable, ValueSource::Member { .. })
                && declarations
                    .iter()
                    .any(|(_, declaration)| declaration.namespaced)
        {
            return None;
        }
        let mut signatures = Vec::new();
        for (_, declaration) in declarations {
            if declaration.implicitly_global {
                return None;
            }
            match self.known_signatures(&declaration, file, document) {
                Some(known) => signatures.extend(known),
                None if is_forward_declaration(&declaration) => {}
                None => return None,
            }
        }
        (!signatures.is_empty()).then_some(signatures)
    }

    fn known_signatures(
        &self,
        declaration: &OwnedDeclaration,
        file: &I,
        document: &SemanticDocument,
    ) -> Option<Vec<OwnedSignature>> {
        if let Some(signature) = &declaration.signature {
            return Some(vec![signature.clone()]);
        }
        if declaration.kind != DeclarationKind::Class {
            return None;
        }
        let class_name = declaration
            .type_object
            .as_deref()
            .unwrap_or(&declaration.name);
        self.closed_owner_chain(
            &ResolvedType::Nominal(class_name.to_string()),
            Some(file),
            Some(document),
        )?;
        let constructors = self.member_declaration_values_for_type(
            &ResolvedType::Nominal(class_name.to_string()),
            "constructor",
            Some(file),
            Some(document),
        );
        if constructors.is_empty() {
            return Some(vec![OwnedSignature {
                label: format!("{class_name}()"),
                parameters: Vec::new(),
            }]);
        }
        constructors
            .into_iter()
            .map(|(_, constructor)| constructor.signature)
            .collect()
    }

    fn callable_declaration_signatures(
        &self,
        declaration: &OwnedDeclaration,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
    ) -> Vec<OwnedSignature> {
        if let Some(signature) = &declaration.signature {
            return vec![signature.clone()];
        }
        if declaration.kind != DeclarationKind::Class {
            return Vec::new();
        }
        let class_name = declaration
            .type_object
            .as_deref()
            .unwrap_or(&declaration.name);
        let constructors = self.member_declaration_values_for_type(
            &ResolvedType::Nominal(class_name.to_string()),
            "constructor",
            override_file,
            override_document,
        );
        if constructors.is_empty() {
            return vec![OwnedSignature {
                label: format!("{class_name}()"),
                parameters: Vec::new(),
            }];
        }
        constructors
            .into_iter()
            .filter_map(|(_, constructor)| constructor.signature)
            .map(|signature| OwnedSignature {
                label: format!(
                    "{class_name}({})",
                    signature
                        .parameters
                        .iter()
                        .map(|parameter| parameter.label.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                parameters: signature.parameters,
            })
            .collect()
    }

    fn callee_declarations(
        &self,
        file: &I,
        document: &SemanticDocument,
        callable: &ValueSource,
    ) -> Vec<(I, OwnedDeclaration)> {
        self.callee_declarations_for_source(file, document, callable, Some(file), Some(document))
    }

    fn callee_declarations_for_source(
        &self,
        file: &I,
        document: &SemanticDocument,
        callable: &ValueSource,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
    ) -> Vec<(I, OwnedDeclaration)> {
        match callable {
            ValueSource::Declaration(range) => document
                .declaration_for_range(range)
                .cloned()
                .map(|declaration| (file.clone(), declaration))
                .into_iter()
                .collect(),
            ValueSource::Workspace(name) => {
                self.named_declarations(name, override_file, override_document, false)
            }
            ValueSource::Member { receiver, name } => self
                .resolve_value_owner_in(
                    Some(file),
                    document,
                    receiver,
                    override_file,
                    override_document,
                    0,
                )
                .map(|owner| {
                    self.member_declaration_values_for_type(
                        &owner,
                        name,
                        override_file,
                        override_document,
                    )
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn named_declarations(
        &self,
        name: &str,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
        include_file_local: bool,
    ) -> Vec<(I, OwnedDeclaration)> {
        self.files
            .iter()
            .flat_map(|file| {
                self.document_with_override(&file.id, override_file, override_document)
                    .declarations_named(name)
                    .filter(move |declaration| {
                        if include_file_local {
                            declaration.file_scope
                        } else {
                            declaration.is_global
                        }
                    })
                    .map(move |declaration| (file.id.clone(), declaration.clone()))
            })
            .collect()
    }

    fn member_declaration_values_for_type(
        &self,
        owner: &ResolvedType<I>,
        name: &str,
        override_file: Option<&I>,
        override_document: Option<&SemanticDocument>,
    ) -> Vec<(I, OwnedDeclaration)> {
        for owner in self.resolved_owner_chain(owner, override_file, override_document) {
            let declarations = self
                .files
                .iter()
                .filter(|file| {
                    !matches!(&owner, ResolvedType::Structural { file: owner_file, .. } if owner_file != &file.id)
                })
                .flat_map(|file| {
                    self.document_with_override(&file.id, override_file, override_document)
                        .declarations
                        .iter()
                        .filter(|declaration| {
                            declaration_owner_matches(&file.id, declaration, &owner)
                                && declaration.name == name
                        })
                        .map(move |declaration| (file.id.clone(), declaration.clone()))
                })
                .collect::<Vec<_>>();
            if !declarations.is_empty() {
                return declarations;
            }
        }
        Vec::new()
    }

    fn document(&self, file: &I) -> Option<&'a SemanticDocument> {
        self.files
            .iter()
            .find(|candidate| &candidate.id == file)
            .map(|candidate| candidate.document)
    }

    fn document_with_override<'b>(
        &'b self,
        file: &I,
        override_file: Option<&I>,
        override_document: Option<&'b SemanticDocument>,
    ) -> &'b SemanticDocument {
        if override_file == Some(file)
            && let Some(document) = override_document
        {
            return document;
        }
        self.files
            .iter()
            .find(|candidate| &candidate.id == file)
            .expect("semantic file disappeared")
            .document
    }
}

fn diagnostic(range: Range<usize>, rule: &'static str, message: String) -> Diagnostic {
    Diagnostic {
        range,
        rule,
        message,
    }
}

fn owner_identity<I: Eq>(file: &I, owner: &ResolvedType<I>) -> Option<TypeIdentity> {
    match owner {
        ResolvedType::Nominal(name) => Some(TypeIdentity::Nominal(name.clone())),
        ResolvedType::Structural {
            file: owner_file,
            range,
        } if owner_file == file => Some(TypeIdentity::Structural(range.clone())),
        ResolvedType::Structural { .. } | ResolvedType::Augmented { .. } => None,
    }
}

fn declaration_owner_matches<I: Eq>(
    file: &I,
    declaration: &OwnedDeclaration,
    owner: &ResolvedType<I>,
) -> bool {
    match (&declaration.owner, owner) {
        (Some(TypeIdentity::Nominal(declaration)), ResolvedType::Nominal(owner)) => {
            declaration == owner
        }
        (
            Some(TypeIdentity::Structural(declaration)),
            ResolvedType::Structural {
                file: owner_file,
                range,
            },
        ) => file == owner_file && declaration == range,
        _ => false,
    }
}

fn unique_value<T: Eq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

fn is_forward_declaration(declaration: &OwnedDeclaration) -> bool {
    declaration.kind == DeclarationKind::Function && declaration.signature.is_none()
}

fn accepts_arguments(signature: &OwnedSignature, arguments: usize) -> bool {
    let variadic = signature
        .parameters
        .iter()
        .any(|parameter| parameter.variadic);
    let declared = signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.variadic)
        .count();
    let required = signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.variadic && !parameter.optional)
        .count();
    arguments >= required && (variadic || arguments <= declared)
}

fn arity_message(signatures: &[OwnedSignature], arguments: usize) -> String {
    let given = format!(
        "{arguments} {} given",
        if arguments == 1 {
            "argument is"
        } else {
            "arguments are"
        }
    );
    let mut labels = signatures
        .iter()
        .map(|signature| signature.label.as_str())
        .collect::<Vec<_>>();
    labels.sort_unstable();
    labels.dedup();
    let [label] = labels.as_slice() else {
        return format!("{given}, which no declaration of this call accepts");
    };
    format!(
        "`{label}` takes {}, but {given}",
        expected_arguments(&signatures[0])
    )
}

fn expected_arguments(signature: &OwnedSignature) -> String {
    let variadic = signature
        .parameters
        .iter()
        .any(|parameter| parameter.variadic);
    let declared = signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.variadic)
        .count();
    let required = signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.variadic && !parameter.optional)
        .count();
    let plural = |count: usize| if count == 1 { "argument" } else { "arguments" };
    if variadic {
        return format!("at least {required} {}", plural(required));
    }
    if required == declared {
        return format!("{declared} {}", plural(declared));
    }
    format!("{required} to {declared} arguments")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_diagnostics(source: &str, expected: &[(&str, &str)]) {
        let document = crate::semantic::analyze(source);
        let file = "test.gnut";
        let workspace = SemanticWorkspace::new([SemanticFile {
            id: file,
            document: &document,
            targets: VmTargets::ALL,
        }]);
        let actual = workspace
            .diagnostics(&file)
            .into_iter()
            .map(|diagnostic| (diagnostic.rule, &source[diagnostic.range]))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn reports_duplicate_declaration_at_second_declaration() {
        assert_diagnostics(
            "void function Example() { local duplicate = 1; local duplicate = 2 }",
            &[(DUPLICATE_DECLARATION_RULE, "duplicate")],
        );
    }

    #[test]
    fn reports_invalid_member_at_member_access() {
        assert_diagnostics(
            "class Expected {} void function Example() { Expected().Missing() }",
            &[(INVALID_MEMBER_RULE, "Missing")],
        );
    }

    #[test]
    fn reports_call_arity_at_call() {
        assert_diagnostics(
            "void function Take(int value) {} void function Example() { Take() }",
            &[(CALL_ARITY_RULE, "()")],
        );
    }

    #[test]
    fn reports_return_type_at_returned_value() {
        assert_diagnostics(
            "class Expected {} class Actual {} Expected function Example() { return Actual() }",
            &[(RETURN_TYPE_RULE, "Actual()")],
        );
    }

    #[test]
    fn reports_initializer_type_at_declaration() {
        assert_diagnostics(
            "class Expected {} class Actual {} void function Example() { Expected value = Actual() }",
            &[(INITIALIZER_TYPE_RULE, "value")],
        );
    }

    #[test]
    fn reports_argument_type_at_argument() {
        assert_diagnostics(
            "class Expected {} class Actual {} void function Take(Expected value) {} void function Example() { Take(Actual()) }",
            &[(ARGUMENT_TYPE_RULE, "Actual()")],
        );
    }
}
