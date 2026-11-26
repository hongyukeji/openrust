use crate::database::eloquent::factories::faker_rng::FakerRandom;

use regex_syntax::ast::{
    Ast, ClassPerl, ClassPerlKind, ClassSet, ClassSetItem, GroupKind, RepetitionKind,
    RepetitionRange, parse::ParserBuilder,
};

use crate::database::{DatabaseError, DatabaseResult};

const LIMIT: usize = 16_384;
const INVALID: DatabaseError = DatabaseError::InvalidQuery("invalid Faker regex pattern");
const UNSUPPORTED: DatabaseError = DatabaseError::InvalidQuery("unsupported Faker regex pattern");
const TOO_LARGE: DatabaseError =
    DatabaseError::InvalidQuery("Faker regex exceeds generation limits");

pub(super) struct RegexPattern {
    pattern: Ast,
}

impl RegexPattern {
    pub(super) fn parse(pattern: &str) -> DatabaseResult<Self> {
        if pattern.len() > LIMIT {
            return Err(TOO_LARGE);
        }
        let pattern = pattern
            .strip_prefix('/')
            .and_then(|inner| inner.strip_suffix('/'))
            .unwrap_or(pattern);
        let pattern = pattern.strip_prefix('^').unwrap_or(pattern);
        let pattern = if let Some(prefix) = pattern.strip_suffix('$')
            && prefix
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'\\')
                .count()
                % 2
                == 0
        {
            prefix
        } else {
            pattern
        };
        let pattern = ParserBuilder::new()
            .nest_limit(32)
            .build()
            .parse(pattern)
            .map_err(|_| INVALID)?;
        let compiled = Self { pattern };
        if Self::max_work(&compiled.pattern)? > LIMIT {
            return Err(TOO_LARGE);
        }
        Ok(compiled)
    }

    pub(super) fn sample(&self, random: &FakerRandom) -> String {
        let mut output = String::new();
        append(
            random,
            &self.pattern,
            &mut output,
            (random.u32(1..=9), random.u32(1..=9)),
        );
        output
    }

    fn bounds(
        kind: &RepetitionKind,
        (star_max, plus_max): (u32, u32),
    ) -> DatabaseResult<(u32, u32)> {
        Ok(match kind {
            RepetitionKind::ZeroOrOne => (0, 1),
            RepetitionKind::ZeroOrMore => (0, star_max),
            RepetitionKind::OneOrMore => (1, plus_max),
            RepetitionKind::Range(RepetitionRange::Exactly(n)) => (*n, *n),
            RepetitionKind::Range(RepetitionRange::Bounded(min, max)) => (*min, *max),
            RepetitionKind::Range(RepetitionRange::AtLeast(_)) => return Err(UNSUPPORTED),
        })
    }

    fn max_work(node: &Ast) -> DatabaseResult<usize> {
        let children = match node {
            Ast::Empty(_) | Ast::Literal(_) | Ast::Dot(_) => 0,
            Ast::ClassPerl(class) if perl_range(class).is_some() => 0,
            Ast::ClassBracketed(class) if !class.negated => {
                let ClassSet::Item(item) = &class.kind else {
                    return Err(UNSUPPORTED);
                };
                class_size(item)?;
                0
            }
            Ast::Group(group) if matches!(group.kind, GroupKind::CaptureIndex(_)) => {
                Self::max_work(&group.ast)?
            }
            Ast::Concat(sequence) => sequence.asts.iter().try_fold(0_usize, |total, node| {
                Ok::<_, DatabaseError>(total.saturating_add(Self::max_work(node)?))
            })?,
            Ast::Alternation(alternatives) => {
                let mut max = 0;
                for node in &alternatives.asts {
                    max = max.max(Self::max_work(node)?);
                }
                max
            }
            Ast::Repetition(repetition) if repetition.greedy => {
                let (_, max) = Self::bounds(&repetition.op.kind, (9, 9))?;
                Self::max_work(&repetition.ast)?.saturating_mul(max as usize)
            }
            _ => return Err(UNSUPPORTED),
        };
        Ok(children.saturating_add(4))
    }
}

fn append(random: &FakerRandom, node: &Ast, output: &mut String, repeat_limits: (u32, u32)) {
    match node {
        Ast::Empty(_) => {}
        Ast::Literal(literal) => output.push(literal.c),
        Ast::Dot(_) => output.push(char::from(random.u8(b'!'..=b'~'))),
        Ast::ClassPerl(class) => {
            let (min, max) = perl_range(class).expect("validated character class");
            output.push(char::from(random.u8(min..=max)));
        }
        Ast::ClassBracketed(class) => {
            let ClassSet::Item(item) = &class.kind else {
                unreachable!("validated character set")
            };
            let index = random.usize(0..class_size(item).expect("validated character set"));
            output.push(class_character(item, index));
        }
        Ast::Group(group) => append(random, &group.ast, output, repeat_limits),
        Ast::Concat(sequence) => {
            for node in &sequence.asts {
                append(random, node, output, repeat_limits);
            }
        }
        Ast::Alternation(alternatives) => {
            append(
                random,
                &alternatives.asts[random.usize(..alternatives.asts.len())],
                output,
                repeat_limits,
            );
        }
        Ast::Repetition(repetition) => {
            let (min, max) = RegexPattern::bounds(&repetition.op.kind, repeat_limits)
                .expect("validated repetition");
            for _ in 0..random.u32(min..=max) {
                append(random, &repetition.ast, output, repeat_limits);
            }
        }
        _ => unreachable!("validated Faker regex"),
    }
}

fn perl_range(class: &ClassPerl) -> Option<(u8, u8)> {
    match (&class.kind, class.negated) {
        (ClassPerlKind::Digit, false) => Some((b'0', b'9')),
        (ClassPerlKind::Word, false) => Some((b'a', b'z')),
        _ => None,
    }
}

fn class_size(item: &ClassSetItem) -> DatabaseResult<usize> {
    match item {
        ClassSetItem::Literal(literal) if literal.c.is_ascii() => Ok(1),
        ClassSetItem::Range(range) if range.start.c.is_ascii() && range.end.c.is_ascii() => {
            Ok(range.end.c as usize - range.start.c as usize + 1)
        }
        ClassSetItem::Union(union) => union
            .items
            .iter()
            .try_fold(0, |total, item| Ok(total + class_size(item)?)),
        _ => Err(UNSUPPORTED),
    }
}

fn class_character(item: &ClassSetItem, mut index: usize) -> char {
    match item {
        ClassSetItem::Literal(literal) => literal.c,
        ClassSetItem::Range(range) => char::from(range.start.c as u8 + index as u8),
        ClassSetItem::Union(union) => {
            for item in &union.items {
                let size = class_size(item).expect("validated character set");
                if index < size {
                    return class_character(item, index);
                }
                index -= size;
            }
            unreachable!("character index belongs to a validated set")
        }
        _ => unreachable!("validated character set"),
    }
}
