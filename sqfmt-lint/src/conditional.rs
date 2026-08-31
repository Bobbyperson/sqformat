//! Conditional compilation regions.
//!
//! Respawn scripts guard code with `#if SERVER` style directives. Squirrel lexes `#` as a
//! script-style comment, so these never reach the parser and every branch is otherwise analyzed as
//! live code. Scanning the source for directive lines recovers the regions and the VM each one can
//! run in, which lets a query ignore declarations that cannot exist alongside it.

use std::ops::Range;

/// The game environments in which a piece of code can run. Unknown conditions allow every VM,
/// ensuring that unrecognized directives never hide anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmTargets(u8);

impl VmTargets {
    pub const SERVER: Self = Self(1);
    pub const CLIENT: Self = Self(2);
    pub const UI: Self = Self(4);
    pub const ALL: Self = Self(7);
    pub const NONE: Self = Self(0);

    pub fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Whether code in these targets can coexist with code in `other`.
    pub fn compatible_with(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub fn is_all(self) -> bool {
        self == Self::ALL
    }

    fn each() -> [Self; 3] {
        [Self::SERVER, Self::CLIENT, Self::UI]
    }

    /// The directive names this describes, for display.
    pub fn names(self) -> Vec<&'static str> {
        [
            (Self::SERVER, "SERVER"),
            (Self::CLIENT, "CLIENT"),
            (Self::UI, "UI"),
        ]
        .into_iter()
        .filter(|(target, _)| self.compatible_with(*target))
        .map(|(_, name)| name)
        .collect()
    }
}

/// A run of source between directives, and the VMs it can run in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConditionalSpan {
    pub range: Range<usize>,
    pub targets: VmTargets,
}

/// The targets that apply at an offset. Code outside every directive runs in any VM.
pub fn targets_at(spans: &[ConditionalSpan], offset: usize) -> VmTargets {
    spans
        .iter()
        .find(|span| span.range.contains(&offset))
        .map_or(VmTargets::ALL, |span| span.targets)
}

/// Scans directive lines and returns the guarded runs of source, in order and non-overlapping.
/// Runs that any VM can reach are omitted.
pub fn conditional_spans(source: &str) -> Vec<ConditionalSpan> {
    let mut spans = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut current = VmTargets::ALL;
    let mut open_from = 0;
    let mut in_block_comment = false;

    for (start, line) in lines(source) {
        let end = start + line.len();
        let trimmed = line.trim_start();
        let directive = (!in_block_comment)
            .then(|| trimmed.strip_prefix('#'))
            .flatten()
            .and_then(parse_directive);
        in_block_comment = block_comment_state(line, in_block_comment);
        let Some(directive) = directive else {
            continue;
        };

        if !current.is_all() && open_from < start {
            spans.push(ConditionalSpan {
                range: open_from..start,
                targets: current,
            });
        }
        match directive {
            Directive::If(condition) => {
                let parent = current;
                stack.push(Frame {
                    parent,
                    taken: certain_targets(&condition),
                });
                current = parent.intersection(possible_targets(&condition));
            }
            Directive::ElseIf(condition) => {
                if let Some(frame) = stack.last_mut() {
                    let parent = frame.parent;
                    current = parent
                        .intersection(possible_targets(&condition))
                        .without(frame.taken);
                    frame.taken = frame.taken.union(certain_targets(&condition));
                }
            }
            Directive::Else => {
                if let Some(frame) = stack.last() {
                    current = frame.parent.without(frame.taken);
                }
            }
            Directive::EndIf => {
                current = stack.pop().map_or(VmTargets::ALL, |frame| frame.parent);
            }
        }
        // The directive line itself belongs to no branch.
        open_from = end;
    }
    if !current.is_all() && open_from < source.len() {
        spans.push(ConditionalSpan {
            range: open_from..source.len(),
            targets: current,
        });
    }
    spans
}

struct Frame {
    parent: VmTargets,
    /// VMs an earlier branch definitely claimed, which a later branch cannot reach.
    taken: VmTargets,
}

/// A run of source and the directive branches containing it, outermost first. Each branch is the
/// start offset of its chain's `#if` line paired with the branch's index in that chain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BranchSpan {
    pub range: Range<usize>,
    pub path: Vec<(usize, usize)>,
}

/// Scans directive lines and returns the runs inside them, in order and non-overlapping. Unlike
/// [`conditional_spans`], this keeps branches whose condition says nothing about the VM, because
/// two branches of one chain still never compile together.
pub fn branch_spans(source: &str) -> Vec<BranchSpan> {
    let mut spans = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut open_from = 0;
    let mut in_block_comment = false;

    for (start, line) in lines(source) {
        let end = start + line.len();
        let trimmed = line.trim_start();
        let directive = (!in_block_comment)
            .then(|| trimmed.strip_prefix('#'))
            .flatten()
            .and_then(parse_directive);
        in_block_comment = block_comment_state(line, in_block_comment);
        let Some(directive) = directive else {
            continue;
        };

        if !stack.is_empty() && open_from < start {
            spans.push(BranchSpan {
                range: open_from..start,
                path: stack.clone(),
            });
        }
        match directive {
            Directive::If(_) => stack.push((start, 0)),
            Directive::ElseIf(_) | Directive::Else => {
                if let Some(branch) = stack.last_mut() {
                    branch.1 += 1;
                }
            }
            Directive::EndIf => {
                stack.pop();
            }
        }
        open_from = end;
    }
    if !stack.is_empty() && open_from < source.len() {
        spans.push(BranchSpan {
            range: open_from..source.len(),
            path: stack,
        });
    }
    spans
}

/// Whether one offset's directive branches are a prefix of the other's, which is when the more
/// deeply guarded position is compiled only alongside the other. Positions under different
/// branches, or under unrelated chains, are not comparable: whether `#if SP` and `#if MP` ever hold
/// together depends on build defines this does not model.
pub fn branches_nested(spans: &[BranchSpan], left: usize, right: usize) -> bool {
    let path_at = |offset: usize| {
        spans
            .iter()
            .find(|span| span.range.contains(&offset))
            .map_or(&[][..], |span| span.path.as_slice())
    };
    let (left, right) = (path_at(left), path_at(right));
    let shared = left.len().min(right.len());
    left[..shared] == right[..shared]
}

enum Directive {
    If(String),
    ElseIf(String),
    Else,
    EndIf,
}

fn parse_directive(rest: &str) -> Option<Directive> {
    let (word, condition) = match rest.find(|character: char| !character.is_alphanumeric()) {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    let condition = condition.trim().to_string();
    match word {
        "if" | "ifdef" => Some(Directive::If(condition)),
        "elseif" | "elif" => Some(Directive::ElseIf(condition)),
        "else" => Some(Directive::Else),
        "endif" => Some(Directive::EndIf),
        _ => None,
    }
}

fn lines(source: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;
    source.split_inclusive('\n').map(move |line| {
        let start = offset;
        offset += line.len();
        (start, line)
    })
}

fn block_comment_state(line: &str, mut open: bool) -> bool {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        match (&bytes[index..index + 2], open) {
            (b"/*", false) => {
                open = true;
                index += 2;
            }
            (b"*/", true) => {
                open = false;
                index += 2;
            }
            (b"//", false) => return open,
            _ => index += 1,
        }
    }
    open
}

/// Three-valued evaluation: a condition may be true, false, or undetermined for a VM because it
/// also depends on identifiers such as `MP` or `DEV` that say nothing about the VM.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Tri {
    True,
    False,
    Unknown,
}

fn possible_targets(condition: &str) -> VmTargets {
    evaluate_targets(condition, |value| value != Tri::False)
}

/// The VMs a condition such as a manifest `RunOn` expression permits.
pub fn condition_targets(condition: &str) -> VmTargets {
    possible_targets(condition)
}

fn certain_targets(condition: &str) -> VmTargets {
    evaluate_targets(condition, |value| value == Tri::True)
}

fn evaluate_targets(condition: &str, keep: impl Fn(Tri) -> bool) -> VmTargets {
    let Some(expression) = parse_condition(condition) else {
        return VmTargets::ALL;
    };
    VmTargets::each()
        .into_iter()
        .filter(|vm| keep(expression.evaluate(*vm)))
        .fold(VmTargets::NONE, VmTargets::union)
}

enum Condition {
    Name(String),
    Not(Box<Condition>),
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
}

impl Condition {
    fn evaluate(&self, vm: VmTargets) -> Tri {
        match self {
            Condition::Name(name) => match name.as_str() {
                "SERVER" => truth(vm == VmTargets::SERVER),
                "CLIENT" => truth(vm == VmTargets::CLIENT),
                "UI" => truth(vm == VmTargets::UI),
                _ => Tri::Unknown,
            },
            Condition::Not(inner) => match inner.evaluate(vm) {
                Tri::True => Tri::False,
                Tri::False => Tri::True,
                Tri::Unknown => Tri::Unknown,
            },
            Condition::And(left, right) => match (left.evaluate(vm), right.evaluate(vm)) {
                (Tri::False, _) | (_, Tri::False) => Tri::False,
                (Tri::True, Tri::True) => Tri::True,
                _ => Tri::Unknown,
            },
            Condition::Or(left, right) => match (left.evaluate(vm), right.evaluate(vm)) {
                (Tri::True, _) | (_, Tri::True) => Tri::True,
                (Tri::False, Tri::False) => Tri::False,
                _ => Tri::Unknown,
            },
        }
    }
}

fn truth(value: bool) -> Tri {
    if value { Tri::True } else { Tri::False }
}

fn parse_condition(condition: &str) -> Option<Condition> {
    let tokens = condition_tokens(condition);
    let mut parser = ConditionParser {
        tokens: &tokens,
        index: 0,
    };
    let expression = parser.or()?;
    parser.finished().then_some(expression)
}

fn condition_tokens(condition: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut name = String::new();
    let mut characters = condition.chars().peekable();
    while let Some(character) = characters.next() {
        if character.is_alphanumeric() || character == '_' {
            name.push(character);
            continue;
        }
        if !name.is_empty() {
            tokens.push(std::mem::take(&mut name));
        }
        match character {
            '(' | ')' | '!' => tokens.push(character.to_string()),
            '&' | '|' => {
                if characters.peek() == Some(&character) {
                    characters.next();
                }
                tokens.push(format!("{character}{character}"));
            }
            character if character.is_whitespace() => {}
            // Anything else, such as a comparison, is not understood.
            _ => tokens.push("?".to_string()),
        }
    }
    if !name.is_empty() {
        tokens.push(name);
    }
    tokens
}

struct ConditionParser<'a> {
    tokens: &'a [String],
    index: usize,
}

impl ConditionParser<'_> {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.index).map(String::as_str)
    }

    fn finished(&self) -> bool {
        self.index == self.tokens.len()
    }

    fn or(&mut self) -> Option<Condition> {
        let mut left = self.and()?;
        while self.peek() == Some("||") {
            self.index += 1;
            left = Condition::Or(Box::new(left), Box::new(self.and()?));
        }
        Some(left)
    }

    fn and(&mut self) -> Option<Condition> {
        let mut left = self.unary()?;
        while self.peek() == Some("&&") {
            self.index += 1;
            left = Condition::And(Box::new(left), Box::new(self.unary()?));
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<Condition> {
        if self.peek() == Some("!") {
            self.index += 1;
            return Some(Condition::Not(Box::new(self.unary()?)));
        }
        let token = self.peek()?;
        if token == "(" {
            self.index += 1;
            let inner = self.or()?;
            (self.peek() == Some(")")).then(|| self.index += 1)?;
            return Some(inner);
        }
        if !token.chars().next()?.is_alphabetic() && !token.starts_with('_') {
            return None;
        }
        let name = token.to_string();
        self.index += 1;
        Some(Condition::Name(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(source: &str, needle: &str) -> VmTargets {
        let spans = conditional_spans(source);
        targets_at(&spans, source.find(needle).unwrap())
    }

    #[test]
    fn reads_vm_targets_from_directives() {
        let source =
            "#if SERVER\nserverOnly\n#endif\nalways\n#if UI || CLIENT\nnotServer\n#endif\n";
        assert_eq!(targets(source, "serverOnly"), VmTargets::SERVER);
        assert_eq!(targets(source, "always"), VmTargets::ALL);
        assert_eq!(
            targets(source, "notServer"),
            VmTargets::CLIENT.union(VmTargets::UI)
        );
    }

    #[test]
    fn handles_else_branches_and_nesting() {
        let source = "#if SERVER\nserverOnly\n#else\nnotServer\n#endif\n#if !UI\n\t#if CLIENT\n\tclientOnly\n\t#endif\nnotUi\n#endif\n";
        assert_eq!(targets(source, "serverOnly"), VmTargets::SERVER);
        assert_eq!(
            targets(source, "notServer"),
            VmTargets::CLIENT.union(VmTargets::UI)
        );
        assert_eq!(targets(source, "clientOnly"), VmTargets::CLIENT);
        assert_eq!(
            targets(source, "notUi"),
            VmTargets::SERVER.union(VmTargets::CLIENT)
        );
    }

    #[test]
    fn keeps_unknown_conditions_open() {
        let source =
            "#if MP\nanyVm\n#else\nalsoAnyVm\n#endif\n#if SERVER && MP\nserverMp\n#endif\n";
        assert_eq!(targets(source, "anyVm"), VmTargets::ALL);
        assert_eq!(targets(source, "alsoAnyVm"), VmTargets::ALL);
        assert_eq!(targets(source, "serverMp"), VmTargets::SERVER);
    }

    #[test]
    fn ignores_directives_inside_block_comments() {
        let source = "/*\n#if SERVER\n*/\nlive\n";
        assert_eq!(targets(source, "live"), VmTargets::ALL);
        assert!(conditional_spans(source).is_empty());
    }

    #[test]
    fn compares_only_nested_directive_branches() {
        let source = concat!(
            "#if SP\n",
            "spOnly\n",
            "#else\n",
            "notSp\n",
            "\t#if DEV\n",
            "\tnotSpDev\n",
            "\t#endif\n",
            "#endif\n",
            "#if MP\n",
            "mpOnly\n",
            "#endif\n",
            "unguarded\n",
        );
        let spans = branch_spans(source);
        let at = |needle: &str| source.find(needle).unwrap();
        let nested = |left: &str, right: &str| branches_nested(&spans, at(left), at(right));

        assert!(nested("unguarded", "spOnly"));
        assert!(nested("notSp", "notSpDev"));
        assert!(!nested("spOnly", "notSp"), "sibling branches never coexist");
        assert!(!nested("spOnly", "mpOnly"), "unrelated chains are unknown");
        assert!(!nested("spOnly", "notSpDev"));
    }

    #[test]
    fn compatibility_needs_a_shared_vm() {
        assert!(!VmTargets::SERVER.compatible_with(VmTargets::CLIENT));
        assert!(VmTargets::SERVER.compatible_with(VmTargets::ALL));
        assert!(
            VmTargets::SERVER
                .union(VmTargets::UI)
                .compatible_with(VmTargets::UI)
        );
    }
}
