//! A stand-in for `/bin/sh` and the POSIX utilities the tests run, for Windows, which has
//! neither. `support::bin` builds it once and links it under each name it answers to
//! (`sh.exe`, `true.exe`, `printf.exe`, ...), so declarations keep their Unix commands.
//!
//! Under any other name, such as `tool.exe` copied next to a script `tool`, it runs that
//! script, as `#!/bin/sh` would.
//!
//! The shell runs the subset of the language the test scripts use: lists with `;`, `&&`,
//! `||` and `&`, pipelines and `!`, `if` and `while`, assignments, quotes and backslashes,
//! `$1`, `$$`, `$!`, `$NAME`, `${NAME}`, `${NAME-word}`, `${NAME+word}`, `$(...)` and
//! `$((...))` with `+` and `-`, and the redirections `>`, `>>`, `<` and `>&2`. Utilities run
//! in process, so `$$` is the shell's own process; other commands are spawned, and `&`
//! spawns the shell again, so `$!` is a real Windows process in the same job.

use std::{
    collections::HashMap,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    process::{self, Child, Command, Stdio},
    thread,
    time::Duration,
};

fn main() {
    let name = env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut args: Vec<String> = env::args().skip(1).collect();
    let name = if name == "fixture" && !args.is_empty() {
        args.remove(0)
    } else if !BUILTINS.contains(&name.as_str()) {
        // Run by a script's own path, as `#!` would: the script is this program's path
        // without `.exe`.
        let script = env::current_exe().unwrap().with_extension("");
        args.insert(0, script.to_string_lossy().into_owned());
        "sh".into()
    } else {
        name
    };
    let mut shell = Shell::new(vec![name.clone()]);
    let mut out = Out::Stdout;
    let mut err = Out::Stderr;
    let status = match shell.utility(&name, &args, &mut Input::Inherit, &mut out, &mut err) {
        Ok(status) | Err(Exit(status)) => status,
    };
    let _ = io::stdout().flush();
    process::exit(status);
}

/// `exit` unwinding to the outermost shell or command substitution.
struct Exit(i32);

type Run = Result<i32, Exit>;

enum Input {
    Inherit,
    Bytes(Vec<u8>),
}

impl Input {
    fn read_all(&mut self) -> Vec<u8> {
        match self {
            Input::Inherit => {
                let mut bytes = Vec::new();
                let _ = io::stdin().read_to_end(&mut bytes);
                bytes
            }
            Input::Bytes(bytes) => std::mem::take(bytes),
        }
    }
}

enum Out {
    Stdout,
    Stderr,
    File(File),
    Buffer(Vec<u8>),
}

impl Out {
    fn stdio(&self) -> io::Result<Stdio> {
        Ok(match self {
            Out::Stdout => Stdio::inherit(),
            Out::Stderr => Stdio::from(io::stderr()),
            Out::File(file) => Stdio::from(file.try_clone()?),
            Out::Buffer(_) => Stdio::piped(),
        })
    }
}

impl Write for Out {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Out::Stdout => io::stdout().write(bytes),
            Out::Stderr => io::stderr().write(bytes),
            Out::File(file) => file.write(bytes),
            Out::Buffer(buffer) => buffer.write(bytes),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Out::Stdout => io::stdout().flush(),
            Out::Stderr => io::stderr().flush(),
            Out::File(file) => file.flush(),
            Out::Buffer(_) => Ok(()),
        }
    }
}

// ---- Syntax ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Part {
    /// Literal text; `quoted` keeps an empty word as one empty field.
    Text(String, bool),
    Param {
        name: String,
        operator: Option<(char, Vec<Part>)>,
        quoted: bool,
    },
    Command(String, bool),
    Arithmetic(String, bool),
}

type Word = Vec<Part>;

#[derive(Clone, Debug)]
enum Token {
    Word(Word),
    Operator(&'static str),
}

fn plain(word: &Word) -> Option<&str> {
    match word.as_slice() {
        [Part::Text(text, false)] => Some(text),
        _ => None,
    }
}

struct Lexer<'a> {
    chars: Vec<char>,
    position: usize,
    source: &'a str,
}

/// A token and the character offsets it spans, for the source of a background command.
type Spanned = (Token, usize, usize);

impl<'a> Lexer<'a> {
    fn tokens(source: &'a str) -> Result<Vec<Spanned>, String> {
        let mut lexer = Lexer {
            chars: source.chars().collect(),
            position: 0,
            source,
        };
        let mut tokens = Vec::new();
        while let Some(token) = lexer.next()? {
            tokens.push(token);
        }
        Ok(tokens)
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.position + offset).copied()
    }

    fn next(&mut self) -> Result<Option<Spanned>, String> {
        while let Some(c) = self.peek(0) {
            if c == ' ' || c == '\t' || c == '\r' {
                self.position += 1;
            } else if c == '\\' && self.peek(1) == Some('\n') {
                self.position += 2;
            } else if c == '#' {
                while self.peek(0).is_some_and(|c| c != '\n') {
                    self.position += 1;
                }
            } else {
                break;
            }
        }
        let start = self.position;
        if self.peek(0).is_none() {
            return Ok(None);
        }
        let operator = |text: &'static str| {
            text.chars()
                .enumerate()
                .all(|(index, expected)| self.peek(index) == Some(expected))
                .then_some(text)
        };
        for text in [
            "&&", "||", ">>", ">&2", "2>&1", ";", "\n", "&", "|", ">", "<",
        ] {
            if let Some(text) = operator(text) {
                self.position += text.chars().count();
                let text = if text == "\n" { ";" } else { text };
                return Ok(Some((Token::Operator(text), start, self.position)));
            }
        }
        let word = self.word()?;
        Ok(Some((Token::Word(word), start, self.position)))
    }

    fn word(&mut self) -> Result<Word, String> {
        let mut parts = Vec::new();
        let mut text = String::new();
        let flush = |text: &mut String, parts: &mut Vec<Part>, quoted: bool| {
            if !text.is_empty() || quoted {
                parts.push(Part::Text(std::mem::take(text), quoted));
            }
        };
        while let Some(c) = self.peek(0) {
            match c {
                ' ' | '\t' | '\r' | '\n' | ';' | '&' | '|' | '<' | '>' => break,
                '\'' => {
                    flush(&mut text, &mut parts, false);
                    self.position += 1;
                    let mut quoted = String::new();
                    loop {
                        match self.peek(0) {
                            None => return Err("unterminated '".into()),
                            Some('\'') => break,
                            Some(c) => quoted.push(c),
                        }
                        self.position += 1;
                    }
                    self.position += 1;
                    parts.push(Part::Text(quoted, true));
                }
                '"' => {
                    flush(&mut text, &mut parts, false);
                    self.position += 1;
                    let mut quoted = String::new();
                    let mut any = false;
                    loop {
                        match self.peek(0) {
                            None => return Err("unterminated \"".into()),
                            Some('"') => break,
                            Some('\\') if matches!(self.peek(1), Some('"' | '\\' | '$' | '`')) => {
                                quoted.push(self.peek(1).unwrap());
                                self.position += 2;
                            }
                            Some('$') if self.expansion_follows() => {
                                if !quoted.is_empty() {
                                    parts.push(Part::Text(std::mem::take(&mut quoted), true));
                                }
                                parts.push(self.expansion(true)?);
                                any = true;
                            }
                            Some(c) => {
                                quoted.push(c);
                                self.position += 1;
                            }
                        }
                    }
                    self.position += 1;
                    if !quoted.is_empty() || !any {
                        parts.push(Part::Text(quoted, true));
                    }
                }
                '\\' => {
                    flush(&mut text, &mut parts, false);
                    self.position += 1;
                    if let Some(c) = self.peek(0) {
                        parts.push(Part::Text(c.to_string(), true));
                        self.position += 1;
                    }
                }
                '$' if self.expansion_follows() => {
                    flush(&mut text, &mut parts, false);
                    parts.push(self.expansion(false)?);
                }
                c => {
                    text.push(c);
                    self.position += 1;
                }
            }
        }
        flush(&mut text, &mut parts, false);
        Ok(parts)
    }

    fn expansion_follows(&self) -> bool {
        self.peek(1)
            .is_some_and(|c| c.is_ascii_alphanumeric() || "_{($!?#@*".contains(c))
    }

    /// The expansion starting at `$`.
    fn expansion(&mut self, quoted: bool) -> Result<Part, String> {
        self.position += 1;
        match self.peek(0) {
            Some('(') if self.peek(1) == Some('(') => {
                self.position += 2;
                let inner = self.until_close(2)?;
                Ok(Part::Arithmetic(inner, quoted))
            }
            Some('(') => {
                self.position += 1;
                let inner = self.until_close(1)?;
                Ok(Part::Command(inner, quoted))
            }
            Some('{') => {
                self.position += 1;
                let mut name = String::new();
                while let Some(c) = self.peek(0) {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        self.position += 1;
                    } else {
                        break;
                    }
                }
                let operator = match self.peek(0) {
                    Some('}') => None,
                    Some(op @ ('-' | '+')) => {
                        self.position += 1;
                        let start = self.position;
                        let mut depth = 0;
                        loop {
                            match self.peek(0) {
                                None => return Err("unterminated ${".into()),
                                Some('{') => depth += 1,
                                Some('}') if depth == 0 => break,
                                Some('}') => depth -= 1,
                                _ => {}
                            }
                            self.position += 1;
                        }
                        let text: String = self.chars[start..self.position].iter().collect();
                        let mut inner = Lexer {
                            chars: text.chars().collect(),
                            position: 0,
                            source: self.source,
                        };
                        Some((op, inner.word()?))
                    }
                    _ => return Err(format!("unsupported ${{{name}...}}")),
                };
                if self.peek(0) != Some('}') {
                    return Err("unterminated ${".into());
                }
                self.position += 1;
                Ok(Part::Param {
                    name,
                    operator,
                    quoted,
                })
            }
            Some(c) if c.is_ascii_digit() || "$!?#@*".contains(c) => {
                self.position += 1;
                Ok(Part::Param {
                    name: c.to_string(),
                    operator: None,
                    quoted,
                })
            }
            _ => {
                let mut name = String::new();
                while let Some(c) = self.peek(0) {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        self.position += 1;
                    } else {
                        break;
                    }
                }
                Ok(Part::Param {
                    name,
                    operator: None,
                    quoted,
                })
            }
        }
    }

    /// The text up to `closing` unbalanced `)`, skipping quoted ones.
    fn until_close(&mut self, closing: usize) -> Result<String, String> {
        let start = self.position;
        let mut depth = 0;
        loop {
            match self.peek(0) {
                None => return Err("unterminated $(".into()),
                Some('\'') => {
                    self.position += 1;
                    while self.peek(0).is_some_and(|c| c != '\'') {
                        self.position += 1;
                    }
                }
                Some('"') => {
                    self.position += 1;
                    while self.peek(0).is_some_and(|c| c != '"') {
                        if self.peek(0) == Some('\\') {
                            self.position += 1;
                        }
                        self.position += 1;
                    }
                }
                Some('(') => depth += 1,
                Some(')') if depth == 0 => {
                    if (1..closing).all(|offset| self.peek(offset) == Some(')')) {
                        let inner = self.chars[start..self.position].iter().collect();
                        self.position += closing;
                        return Ok(inner);
                    }
                    return Err("unbalanced )".into());
                }
                Some(')') => depth -= 1,
                _ => {}
            }
            self.position += 1;
        }
    }
}

#[derive(Debug)]
enum Redirect {
    Out(Word),
    Append(Word),
    In(Word),
    OutToErr,
    ErrToOut,
}

#[derive(Debug)]
enum Node {
    Simple(Vec<Word>, Vec<Redirect>),
    If(List, List, Option<List>),
    While(List, List),
}

#[derive(Debug)]
struct Pipeline {
    negate: bool,
    commands: Vec<Node>,
}

#[derive(Debug)]
struct AndOr {
    first: Pipeline,
    rest: Vec<(&'static str, Pipeline)>,
    background: bool,
    /// The source text, which `&` hands to a new shell.
    source: String,
}

type List = Vec<AndOr>;

struct Parser {
    tokens: Vec<Spanned>,
    position: usize,
    chars: Vec<char>,
}

impl Parser {
    fn parse(source: &str) -> Result<List, String> {
        let mut parser = Parser {
            tokens: Lexer::tokens(source)?,
            position: 0,
            chars: source.chars().collect(),
        };
        let list = parser.list(&[])?;
        if parser.position < parser.tokens.len() {
            return Err(format!("unexpected {:?}", parser.tokens[parser.position].0));
        }
        Ok(list)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position).map(|(token, _, _)| token)
    }

    fn at(&self, operator: &str) -> bool {
        matches!(self.peek(), Some(Token::Operator(found)) if *found == operator)
    }

    fn keyword(&self) -> Option<&str> {
        match self.peek() {
            Some(Token::Word(word)) => plain(word),
            _ => None,
        }
    }

    fn expect(&mut self, keyword: &str) -> Result<(), String> {
        if self.keyword() != Some(keyword) {
            return Err(format!("expected {keyword}"));
        }
        self.position += 1;
        Ok(())
    }

    fn list(&mut self, terminators: &[&str]) -> Result<List, String> {
        let mut list = Vec::new();
        loop {
            while self.at(";") {
                self.position += 1;
            }
            if self.peek().is_none() || self.keyword().is_some_and(|k| terminators.contains(&k)) {
                return Ok(list);
            }
            let start = self.tokens[self.position].1;
            let mut and_or = self.and_or()?;
            let end = self.tokens[self.position - 1].2;
            and_or.source = self.chars[start..end].iter().collect();
            match self.peek() {
                Some(Token::Operator("&")) => {
                    and_or.background = true;
                    self.position += 1;
                }
                Some(Token::Operator(";")) | None => {}
                Some(Token::Word(_))
                    if self.keyword().is_some_and(|k| terminators.contains(&k)) => {}
                Some(token) => return Err(format!("unexpected {token:?}")),
            }
            list.push(and_or);
        }
    }

    fn and_or(&mut self) -> Result<AndOr, String> {
        let first = self.pipeline()?;
        let mut rest = Vec::new();
        while let Some(Token::Operator(operator @ ("&&" | "||"))) = self.peek() {
            let operator = *operator;
            self.position += 1;
            while self.at(";") {
                self.position += 1;
            }
            rest.push((operator, self.pipeline()?));
        }
        Ok(AndOr {
            first,
            rest,
            background: false,
            source: String::new(),
        })
    }

    fn pipeline(&mut self) -> Result<Pipeline, String> {
        let negate = self.keyword() == Some("!");
        if negate {
            self.position += 1;
        }
        let mut commands = vec![self.command()?];
        while self.at("|") {
            self.position += 1;
            commands.push(self.command()?);
        }
        Ok(Pipeline { negate, commands })
    }

    fn command(&mut self) -> Result<Node, String> {
        match self.keyword() {
            Some("if") => {
                self.position += 1;
                let condition = self.list(&["then"])?;
                self.expect("then")?;
                let body = self.list(&["else", "fi"])?;
                let otherwise = if self.keyword() == Some("else") {
                    self.position += 1;
                    Some(self.list(&["fi"])?)
                } else {
                    None
                };
                self.expect("fi")?;
                return Ok(Node::If(condition, body, otherwise));
            }
            Some("while") => {
                self.position += 1;
                let condition = self.list(&["do"])?;
                self.expect("do")?;
                let body = self.list(&["done"])?;
                self.expect("done")?;
                return Ok(Node::While(condition, body));
            }
            _ => {}
        }
        let mut words = Vec::new();
        let mut redirects = Vec::new();
        loop {
            match self.peek().cloned() {
                Some(Token::Word(word)) => {
                    words.push(word);
                    self.position += 1;
                }
                Some(Token::Operator(operator @ (">" | ">>" | "<"))) => {
                    self.position += 1;
                    let Some(Token::Word(target)) = self.peek().cloned() else {
                        return Err(format!("{operator} needs a file"));
                    };
                    self.position += 1;
                    redirects.push(match operator {
                        ">" => Redirect::Out(target),
                        ">>" => Redirect::Append(target),
                        _ => Redirect::In(target),
                    });
                }
                Some(Token::Operator(">&2")) => {
                    self.position += 1;
                    redirects.push(Redirect::OutToErr);
                }
                Some(Token::Operator("2>&1")) => {
                    self.position += 1;
                    redirects.push(Redirect::ErrToOut);
                }
                _ => break,
            }
        }
        if words.is_empty() && redirects.is_empty() {
            return Err(format!("expected a command, found {:?}", self.peek()));
        }
        Ok(Node::Simple(words, redirects))
    }
}

// ---- Execution ------------------------------------------------------------------------

struct Shell {
    /// `$0`, `$1`, ...
    args: Vec<String>,
    variables: HashMap<String, String>,
    jobs: Vec<Child>,
    last_job: Option<u32>,
    status: i32,
    errexit: bool,
    /// Inside an `if` or `while` condition, where `set -e` does not apply.
    condition: usize,
}

impl Shell {
    fn new(args: Vec<String>) -> Self {
        Shell {
            args,
            variables: HashMap::new(),
            jobs: Vec::new(),
            last_job: None,
            status: 0,
            errexit: false,
            condition: 0,
        }
    }

    fn script(&mut self, source: &str, input: &mut Input, out: &mut Out, err: &mut Out) -> Run {
        match Parser::parse(source) {
            Ok(list) => self.list(&list, input, out, err),
            Err(error) => {
                let _ = writeln!(err, "sh: syntax error: {error}");
                Ok(2)
            }
        }
    }

    fn list(&mut self, list: &List, input: &mut Input, out: &mut Out, err: &mut Out) -> Run {
        let mut status = 0;
        for and_or in list {
            if and_or.background {
                status = self.background(&and_or.source, err);
            } else {
                status = self.and_or(and_or, input, out, err)?;
                if self.errexit
                    && self.condition == 0
                    && status != 0
                    && and_or.rest.is_empty()
                    && !and_or.first.negate
                {
                    return Err(Exit(status));
                }
            }
            self.status = status;
        }
        Ok(status)
    }

    fn and_or(&mut self, and_or: &AndOr, input: &mut Input, out: &mut Out, err: &mut Out) -> Run {
        let mut status = self.pipeline(&and_or.first, input, out, err)?;
        for (operator, pipeline) in &and_or.rest {
            if (*operator == "&&") == (status == 0) {
                status = self.pipeline(pipeline, input, out, err)?;
            }
        }
        Ok(status)
    }

    fn pipeline(
        &mut self,
        pipeline: &Pipeline,
        input: &mut Input,
        out: &mut Out,
        err: &mut Out,
    ) -> Run {
        let mut piped: Option<Vec<u8>> = None;
        let mut status = 0;
        let count = pipeline.commands.len();
        for (index, command) in pipeline.commands.iter().enumerate() {
            let mut stage = piped.take().map(Input::Bytes);
            let input = match &mut stage {
                Some(stage) => stage,
                None => &mut *input,
            };
            if index + 1 < count {
                let mut buffer = Out::Buffer(Vec::new());
                self.node(command, input, &mut buffer, err)?;
                let Out::Buffer(bytes) = buffer else {
                    unreachable!()
                };
                piped = Some(bytes);
            } else {
                status = self.node(command, input, out, err)?;
            }
        }
        Ok(if pipeline.negate {
            i32::from(status == 0)
        } else {
            status
        })
    }

    fn node(&mut self, node: &Node, input: &mut Input, out: &mut Out, err: &mut Out) -> Run {
        match node {
            Node::Simple(words, redirects) => self.simple(words, redirects, input, out, err),
            Node::If(condition, body, otherwise) => {
                self.condition += 1;
                let test = self.list(condition, input, out, err);
                self.condition -= 1;
                if test? == 0 {
                    self.list(body, input, out, err)
                } else if let Some(otherwise) = otherwise {
                    self.list(otherwise, input, out, err)
                } else {
                    Ok(0)
                }
            }
            Node::While(condition, body) => {
                let mut status = 0;
                loop {
                    self.condition += 1;
                    let test = self.list(condition, input, out, err);
                    self.condition -= 1;
                    if test? != 0 {
                        return Ok(status);
                    }
                    status = self.list(body, input, out, err)?;
                }
            }
        }
    }

    fn simple(
        &mut self,
        words: &[Word],
        redirects: &[Redirect],
        input: &mut Input,
        out: &mut Out,
        err: &mut Out,
    ) -> Run {
        let mut fields = Vec::new();
        for word in words {
            fields.extend(self.expand(word)?);
        }
        let mut assignments = Vec::new();
        while let Some(field) = fields.first() {
            match field.split_once('=') {
                Some((name, value))
                    if !name.is_empty()
                        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !name.starts_with(|c: char| c.is_ascii_digit()) =>
                {
                    assignments.push((name.to_owned(), value.to_owned()));
                    fields.remove(0);
                }
                _ => break,
            }
        }
        if fields.is_empty() {
            self.variables.extend(assignments.clone());
        }
        let mut stdout = None;
        let mut stdin = None;
        let mut to_err = false;
        for redirect in redirects {
            let opened = match redirect {
                Redirect::Out(target) => {
                    let path = self.path_word(target)?;
                    File::create(&path).map(|file| stdout = Some(Out::File(file)))
                }
                Redirect::Append(target) => {
                    let path = self.path_word(target)?;
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .map(|file| stdout = Some(Out::File(file)))
                }
                Redirect::In(target) => {
                    let path = self.path_word(target)?;
                    fs::read(&path).map(|bytes| stdin = Some(Input::Bytes(bytes)))
                }
                Redirect::OutToErr => {
                    to_err = true;
                    Ok(())
                }
                // Every caller here keeps stderr separate; nothing to merge into.
                Redirect::ErrToOut => Ok(()),
            };
            if let Err(error) = opened {
                let _ = writeln!(err, "sh: {error}");
                return Ok(1);
            }
        }
        let Some((name, args)) = fields.split_first() else {
            return Ok(0);
        };
        let mut redirected = stdin;
        let input = match &mut redirected {
            Some(redirected) => redirected,
            None => input,
        };
        let status = match (stdout, to_err) {
            (Some(mut file), _) => self.command(name, args, &assignments, input, &mut file, err),
            (None, true) => {
                let mut copy = match err {
                    Out::Stdout => Out::Stdout,
                    Out::Stderr => Out::Stderr,
                    Out::File(file) => Out::File(file.try_clone().unwrap()),
                    Out::Buffer(_) => Out::Stderr,
                };
                self.command(name, args, &assignments, input, &mut copy, err)
            }
            (None, false) => self.command(name, args, &assignments, input, out, err),
        };
        let _ = out.flush();
        status
    }

    fn path_word(&mut self, word: &Word) -> Result<String, Exit> {
        let path = self.expand(word)?.concat();
        Ok(if path == "/dev/null" {
            "NUL".into()
        } else {
            path
        })
    }

    fn command(
        &mut self,
        name: &str,
        args: &[String],
        assignments: &[(String, String)],
        input: &mut Input,
        out: &mut Out,
        err: &mut Out,
    ) -> Run {
        if BUILTINS.contains(&name) {
            return self.utility(name, args, input, out, err);
        }
        let mut command = Command::new(name);
        command.args(args).envs(assignments.iter().cloned());
        self.spawn_external(command, input, out, err)
    }

    fn spawn_external(
        &mut self,
        mut command: Command,
        input: &mut Input,
        out: &mut Out,
        err: &mut Out,
    ) -> Run {
        let stdin = match input {
            Input::Inherit => Stdio::inherit(),
            Input::Bytes(_) => Stdio::piped(),
        };
        let spawned = (|| {
            command
                .stdin(stdin)
                .stdout(out.stdio()?)
                .stderr(err.stdio()?);
            command.envs(self.variables.clone());
            let mut child = command.spawn()?;
            if let (Input::Bytes(bytes), Some(mut pipe)) = (input, child.stdin.take()) {
                let bytes = std::mem::take(bytes);
                thread::spawn(move || {
                    let _ = pipe.write_all(&bytes);
                });
            }
            let output = child.wait_with_output()?;
            Ok::<_, io::Error>(output)
        })();
        match spawned {
            Ok(output) => {
                let _ = out.write_all(&output.stdout);
                let _ = err.write_all(&output.stderr);
                Ok(output.status.code().unwrap_or(1))
            }
            Err(error) => {
                let _ = writeln!(
                    err,
                    "sh: {}: {error}",
                    command.get_program().to_string_lossy()
                );
                Ok(127)
            }
        }
    }

    /// Run `source` in a new shell process, as `&` does.
    fn background(&mut self, source: &str, err: &mut Out) -> i32 {
        let shell = env::current_exe()
            .unwrap()
            .with_file_name(format!("sh{}", env::consts::EXE_SUFFIX));
        let spawned = Command::new(shell)
            .arg("-c")
            .arg(source)
            .args(&self.args)
            .envs(self.variables.clone())
            .stdin(Stdio::null())
            .spawn();
        match spawned {
            Ok(child) => {
                self.last_job = Some(child.id());
                self.jobs.push(child);
                0
            }
            Err(error) => {
                let _ = writeln!(err, "sh: {error}");
                1
            }
        }
    }

    fn variable(&self, name: &str) -> Option<String> {
        if let Ok(index) = name.parse::<usize>() {
            return self.args.get(index).cloned();
        }
        match name {
            "$" => Some(process::id().to_string()),
            "!" => self.last_job.map(|pid| pid.to_string()),
            "?" => Some(self.status.to_string()),
            "#" => Some(self.args.len().saturating_sub(1).to_string()),
            "@" | "*" => Some(self.args.get(1..).unwrap_or_default().join(" ")),
            "PWD" => self
                .variables
                .get(name)
                .cloned()
                .or_else(|| env::current_dir().ok().map(|dir| dir.display().to_string())),
            _ => self
                .variables
                .get(name)
                .cloned()
                .or_else(|| env::var(name).ok()),
        }
    }

    /// Fields after parameter, command and arithmetic expansion and splitting.
    fn expand(&mut self, word: &Word) -> Result<Vec<String>, Exit> {
        let mut fields = Vec::new();
        let mut current = String::new();
        let mut started = false;
        for part in word {
            // Quoted "$@" expands to one field per argument, preserving empty fields and
            // attaching surrounding word fragments to the first/last argument respectively.
            if let Part::Param { name, operator: None, quoted: true } = part
                && name == "@"
            {
                let mut arguments = self.args.iter().skip(1).peekable();
                while let Some(argument) = arguments.next() {
                    current.push_str(argument);
                    started = true;
                    if arguments.peek().is_some() {
                        fields.push(std::mem::take(&mut current));
                    }
                }
                continue;
            }
            let (value, quoted) = match part {
                Part::Text(text, quoted) => {
                    current.push_str(text);
                    started |= *quoted || !text.is_empty();
                    continue;
                }
                Part::Param {
                    name,
                    operator,
                    quoted,
                } => {
                    let value = self.variable(name);
                    let value = match (operator, value) {
                        (None, value) => value.unwrap_or_default(),
                        (Some(('-', word)), None) | (Some(('+', word)), Some(_)) => {
                            self.expand(word)?.join(" ")
                        }
                        (Some(('-', _)), Some(value)) => value,
                        (Some(_), _) => String::new(),
                    };
                    (value, *quoted)
                }
                Part::Command(source, quoted) => {
                    let mut buffer = Out::Buffer(Vec::new());
                    let mut err = Out::Stderr;
                    let mut nested = Shell::new(self.args.clone());
                    nested.variables = self.variables.clone();
                    let status =
                        match nested.script(source, &mut Input::Inherit, &mut buffer, &mut err) {
                            Ok(status) | Err(Exit(status)) => status,
                        };
                    self.status = status;
                    let Out::Buffer(bytes) = buffer else {
                        unreachable!()
                    };
                    let text = String::from_utf8_lossy(&bytes);
                    (text.trim_end_matches('\n').to_owned(), *quoted)
                }
                Part::Arithmetic(expression, quoted) => {
                    (self.arithmetic(expression).to_string(), *quoted)
                }
            };
            if quoted {
                current.push_str(&value);
                started = true;
            } else {
                let mut pieces = value.split_whitespace().peekable();
                if value.starts_with(char::is_whitespace) && started {
                    fields.push(std::mem::take(&mut current));
                    started = false;
                }
                while let Some(piece) = pieces.next() {
                    current.push_str(piece);
                    started = true;
                    if pieces.peek().is_some() {
                        fields.push(std::mem::take(&mut current));
                    }
                }
                if value.ends_with(char::is_whitespace) && started {
                    fields.push(std::mem::take(&mut current));
                    started = false;
                }
            }
        }
        if started {
            fields.push(current);
        }
        Ok(fields)
    }

    /// Integers, variables, `+` and `-`, evaluated left to right.
    fn arithmetic(&self, expression: &str) -> i64 {
        let mut total = 0;
        let mut sign = 1;
        let mut term = String::new();
        let value = |term: &str| {
            let term = term.trim().trim_start_matches('$');
            term.parse::<i64>().unwrap_or_else(|_| {
                self.variable(term)
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or(0)
            })
        };
        for c in expression.chars().chain([' ']) {
            if c == '+' || c == '-' || (c == ' ' && !term.trim().is_empty()) {
                if !term.trim().is_empty() {
                    total += sign * value(&term);
                    term.clear();
                }
                if c != ' ' {
                    sign = if c == '-' { -1 } else { 1 };
                }
            } else if c != ' ' {
                term.push(c);
            }
        }
        total
    }

    // ---- Utilities ----------------------------------------------------------------------

    fn utility(
        &mut self,
        name: &str,
        args: &[String],
        input: &mut Input,
        out: &mut Out,
        err: &mut Out,
    ) -> Run {
        let fail = |err: &mut Out, message: String| {
            let _ = writeln!(err, "{name}: {message}");
            1
        };
        let status = match name {
            "sh" => return Ok(self.sh(args, input, out, err)),
            ":" | "true" => 0,
            "false" => 1,
            "exit" => {
                let code = args.first().and_then(|code| code.parse().ok());
                return Err(Exit(code.unwrap_or(self.status)));
            }
            "set" => {
                for flag in args {
                    if let Some(flags) = flag.strip_prefix('-') {
                        self.errexit |= flags.contains('e');
                    } else if let Some(flags) = flag.strip_prefix('+') {
                        self.errexit &= !flags.contains('e');
                    }
                }
                0
            }
            "echo" => {
                let _ = writeln!(out, "{}", args.join(" "));
                0
            }
            "printf" => match args.split_first() {
                Some((format, args)) => {
                    let _ = out.write_all(&printf(format, args));
                    0
                }
                None => fail(err, "missing format".into()),
            },
            "pwd" => {
                let _ = writeln!(out, "{}", env::current_dir().unwrap().display());
                0
            }
            "env" => {
                let mut variables: Vec<_> = env::vars_os().collect();
                variables.sort();
                for (key, value) in variables {
                    let _ = writeln!(out, "{}={}", key.to_string_lossy(), value.to_string_lossy());
                }
                0
            }
            "cat" => {
                let mut status = 0;
                if args.is_empty() {
                    let _ = out.write_all(&input.read_all());
                }
                for path in args {
                    match fs::read(path) {
                        Ok(bytes) => {
                            let _ = out.write_all(&bytes);
                        }
                        Err(error) => status = fail(err, format!("{path}: {error}")),
                    }
                }
                status
            }
            "head" => match args {
                [flag, count, rest @ ..] if flag == "-c" => {
                    let count: usize = count.parse().unwrap_or(0);
                    let bytes = match rest.first().map(String::as_str) {
                        Some("/dev/zero") => vec![0; count],
                        Some(path) => fs::read(path).unwrap_or_default(),
                        None => input.read_all(),
                    };
                    let _ = out.write_all(&bytes[..count.min(bytes.len())]);
                    0
                }
                _ => fail(err, "only -c COUNT [FILE] is supported".into()),
            },
            "tr" => match args {
                [from, to] => {
                    let (from, to) = (printf(from, &[]), printf(to, &[]));
                    let bytes: Vec<u8> = input
                        .read_all()
                        .into_iter()
                        .map(|byte| match from.iter().position(|&b| b == byte) {
                            Some(index) => *to.get(index).or(to.last()).unwrap_or(&byte),
                            None => byte,
                        })
                        .collect();
                    let _ = out.write_all(&bytes);
                    0
                }
                _ => fail(err, "only SET1 SET2 is supported".into()),
            },
            "cksum" => {
                let bytes = input.read_all();
                let _ = writeln!(out, "{} {}", cksum(&bytes), bytes.len());
                0
            }
            "grep" => {
                let (mut quiet, mut count, mut most) = (false, false, usize::MAX);
                let mut options = args.iter();
                let mut operands = Vec::new();
                while let Some(arg) = options.next() {
                    match arg.as_str() {
                        "-q" => quiet = true,
                        "-c" => count = true,
                        "-m" => {
                            most = options.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                        }
                        _ => operands.push(arg),
                    }
                }
                let mut operands = operands.into_iter();
                let Some(pattern) = operands.next() else {
                    return Ok(fail(err, "missing pattern".into()));
                };
                let text = match operands.next() {
                    Some(path) => match fs::read(path) {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            fail(err, format!("{path}: {error}"));
                            return Ok(2);
                        }
                    },
                    None => input.read_all(),
                };
                let text = String::from_utf8_lossy(&text);
                let matching: Vec<_> = text
                    .lines()
                    .filter(|line| matches(pattern, line))
                    .take(most)
                    .collect();
                if count {
                    let _ = writeln!(out, "{}", matching.len());
                } else if !quiet {
                    for line in &matching {
                        let _ = writeln!(out, "{line}");
                    }
                }
                i32::from(matching.is_empty())
            }
            "ls" => {
                let directory = args.iter().find(|arg| !arg.starts_with('-'));
                match fs::read_dir(directory.map_or(".", String::as_str)) {
                    Ok(entries) => {
                        let mut names: Vec<_> = entries
                            .filter_map(|entry| entry.ok())
                            .map(|entry| entry.file_name().to_string_lossy().into_owned())
                            .filter(|name| !name.starts_with('.'))
                            .collect();
                        names.sort();
                        for name in names {
                            let _ = writeln!(out, "{name}");
                        }
                        0
                    }
                    Err(error) => fail(err, error.to_string()),
                }
            }
            "touch" => {
                let mut status = 0;
                for path in args {
                    if let Err(error) = OpenOptions::new().create(true).append(true).open(path) {
                        status = fail(err, format!("{path}: {error}"));
                    }
                }
                status
            }
            "rm" => {
                let force = args
                    .iter()
                    .any(|arg| arg.starts_with('-') && arg.contains('f'));
                let recursive = args
                    .iter()
                    .any(|arg| arg.starts_with('-') && arg.contains('r'));
                let mut status = 0;
                for path in args.iter().filter(|arg| !arg.starts_with('-')) {
                    let removed = if recursive && Path::new(path).is_dir() {
                        fs::remove_dir_all(path)
                    } else {
                        fs::remove_file(path)
                    };
                    if let Err(error) = removed
                        && !(force && error.kind() == io::ErrorKind::NotFound)
                    {
                        status = fail(err, format!("{path}: {error}"));
                    }
                }
                status
            }
            "mkdir" => {
                let parents = args.iter().any(|arg| arg == "-p");
                let mut status = 0;
                for path in args.iter().filter(|arg| !arg.starts_with('-')) {
                    let created = if parents {
                        fs::create_dir_all(path)
                    } else {
                        fs::create_dir(path)
                    };
                    if let Err(error) = created {
                        status = fail(err, format!("{path}: {error}"));
                    }
                }
                status
            }
            "cd" => match env::set_current_dir(args.first().map_or(".", String::as_str)) {
                Ok(()) => 0,
                Err(error) => fail(err, error.to_string()),
            },
            "sleep" => match args.first().and_then(|seconds| seconds.parse::<f64>().ok()) {
                Some(seconds) => {
                    thread::sleep(Duration::from_secs_f64(seconds));
                    0
                }
                None => fail(err, "invalid time".into()),
            },
            "test" => test(args),
            "[" => match args.split_last() {
                Some((last, args)) if last == "]" => test(args),
                _ => fail(err, "missing ]".into()),
            },
            "wait" => {
                let mut status = 0;
                for mut job in self.jobs.drain(..) {
                    status = job
                        .wait()
                        .ok()
                        .and_then(|status| status.code())
                        .unwrap_or(1);
                }
                status
            }
            "trap" => match args {
                [action, signals @ ..] if action.is_empty() => {
                    if signals
                        .iter()
                        .any(|signal| matches!(signal.as_str(), "TERM" | "INT"))
                    {
                        ignore_interrupts();
                    }
                    0
                }
                _ => fail(err, "only `trap '' SIGNAL` is supported".into()),
            },
            "exec" => {
                let Some((name, args)) = args.split_first() else {
                    return Ok(0);
                };
                let status = self.command(name, args, &[], input, out, err)?;
                let _ = out.flush();
                return Err(Exit(status));
            }
            _ => {
                let _ = writeln!(err, "fixture: unknown utility {name}");
                127
            }
        };
        Ok(status)
    }

    /// `sh -c SCRIPT [NAME [ARG...]]` or `sh FILE [ARG...]`, in this process.
    fn sh(&mut self, args: &[String], input: &mut Input, out: &mut Out, err: &mut Out) -> i32 {
        let (source, positional) = match args {
            [flag, script, positional @ ..] if flag == "-c" => {
                let mut positional = positional.to_vec();
                if positional.is_empty() {
                    positional.push("sh".into());
                }
                (script.clone(), positional)
            }
            [file, rest @ ..] => match fs::read_to_string(file) {
                Ok(source) => {
                    let mut positional = vec![file.clone()];
                    positional.extend_from_slice(rest);
                    (source, positional)
                }
                Err(error) => {
                    let _ = writeln!(err, "sh: {file}: {error}");
                    return 127;
                }
            },
            [] => {
                let source = String::from_utf8_lossy(&input.read_all()).into_owned();
                (source, vec!["sh".into()])
            }
        };
        let mut shell = Shell::new(positional);
        shell.variables = self.variables.clone();
        let status = match shell.script(&source, input, out, err) {
            Ok(status) | Err(Exit(status)) => status,
        };
        // A script's background jobs outlive it, as with a real shell.
        self.jobs.append(&mut shell.jobs);
        status
    }
}

const BUILTINS: &[&str] = &[
    ":", "[", "cat", "cd", "cksum", "echo", "env", "exec", "exit", "false", "grep", "head", "ls",
    "mkdir", "printf", "pwd", "rm", "set", "sh", "sleep", "test", "touch", "tr", "trap", "true",
    "wait",
];

/// `printf` with `\` escapes, including octal bytes, and `%s`, `%d` and `%%`. The format is
/// reused while arguments remain.
fn printf(format: &str, args: &[String]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut next = 0;
    loop {
        let mut consumed = false;
        let chars: Vec<char> = format.chars().collect();
        let mut index = 0;
        while index < chars.len() {
            match chars[index] {
                '\\' if index + 1 < chars.len() => {
                    index += 1;
                    let escaped = match chars[index] {
                        'n' => Some(b'\n'),
                        't' => Some(b'\t'),
                        'r' => Some(b'\r'),
                        'a' => Some(7),
                        'b' => Some(8),
                        'f' => Some(12),
                        'v' => Some(11),
                        '\\' => Some(b'\\'),
                        '0'..='7' => {
                            let mut value = 0_u32;
                            let mut digits = 0;
                            while digits < 3
                                && index < chars.len()
                                && ('0'..='7').contains(&chars[index])
                            {
                                value = value * 8 + chars[index].to_digit(8).unwrap();
                                index += 1;
                                digits += 1;
                            }
                            index -= 1;
                            Some(value as u8)
                        }
                        _ => None,
                    };
                    match escaped {
                        Some(byte) => output.push(byte),
                        None => {
                            output.push(b'\\');
                            output.extend(chars[index].to_string().as_bytes());
                        }
                    }
                }
                '%' if index + 1 < chars.len() => {
                    index += 1;
                    match chars[index] {
                        '%' => output.push(b'%'),
                        's' | 'd' | 'i' => {
                            let value = args.get(next).cloned().unwrap_or_default();
                            next += 1;
                            consumed = true;
                            let value = if chars[index] == 's' {
                                value
                            } else {
                                value.trim().parse::<i64>().unwrap_or(0).to_string()
                            };
                            output.extend(value.as_bytes());
                        }
                        other => output.extend(format!("%{other}").as_bytes()),
                    }
                }
                c => output.extend(c.to_string().as_bytes()),
            }
            index += 1;
        }
        if !consumed || next >= args.len() {
            return output;
        }
    }
}

/// POSIX `test`: 0 when true, 1 when false, 2 for an unsupported expression.
fn test(args: &[String]) -> i32 {
    let truth = |value: bool| i32::from(!value);
    let path = |path: &String| Path::new(path).to_owned();
    match args {
        [] => 1,
        [value] => truth(!value.is_empty()),
        [not, rest @ ..] if not == "!" => match test(rest) {
            2 => 2,
            status => i32::from(status == 0),
        },
        [operator, operand] => match operator.as_str() {
            "-e" => truth(path(operand).exists()),
            "-f" => truth(path(operand).is_file()),
            "-d" => truth(path(operand).is_dir()),
            "-s" => truth(fs::metadata(operand).is_ok_and(|m| m.len() > 0)),
            "-x" => truth(path(operand).is_file()),
            "-z" => truth(operand.is_empty()),
            "-n" => truth(!operand.is_empty()),
            _ => 2,
        },
        [left, operator, right] => {
            let number = |value: &String| value.trim().parse::<i64>();
            let compare = |op: fn(&i64, &i64) -> bool| match (number(left), number(right)) {
                (Ok(left), Ok(right)) => truth(op(&left, &right)),
                _ => 2,
            };
            match operator.as_str() {
                "=" | "==" => truth(left == right),
                "!=" => truth(left != right),
                "-eq" => compare(i64::eq),
                "-ne" => compare(i64::ne),
                "-lt" => compare(i64::lt),
                "-le" => compare(i64::le),
                "-gt" => compare(i64::gt),
                "-ge" => compare(i64::ge),
                "-ef" => truth(match (fs::canonicalize(left), fs::canonicalize(right)) {
                    (Ok(left), Ok(right)) => left == right,
                    _ => false,
                }),
                _ => 2,
            }
        }
        _ => 2,
    }
}

/// A basic regular expression with `^`, `$`, `.` and `\` escapes, unanchored by default.
fn matches(pattern: &str, line: &str) -> bool {
    let (anchored, pattern) = match pattern.strip_prefix('^') {
        Some(rest) => (true, rest),
        None => (false, pattern),
    };
    let (end, pattern) = match pattern.strip_suffix('$') {
        Some(rest) if !rest.ends_with('\\') => (true, rest),
        _ => (false, pattern),
    };
    let mut atoms = Vec::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        atoms.push(match c {
            '\\' => chars.next().map(Some).unwrap_or(Some('\\')),
            '.' => None,
            c => Some(c),
        });
    }
    let line: Vec<char> = line.chars().collect();
    let at = |start: usize| {
        start + atoms.len() <= line.len()
            && atoms
                .iter()
                .zip(&line[start..])
                .all(|(atom, c)| atom.is_none_or(|atom| atom == *c))
            && (!end || start + atoms.len() == line.len())
    };
    if anchored {
        at(0)
    } else {
        (0..=line.len()).any(at)
    }
}

/// The POSIX `cksum` CRC: CRC-32 over the bytes and then their length, complemented.
fn cksum(bytes: &[u8]) -> u32 {
    let mut crc = 0_u32;
    let mut update = |byte: u8| {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    };
    for &byte in bytes {
        update(byte);
    }
    let mut length = bytes.len();
    while length > 0 {
        update(length as u8);
        length >>= 8;
    }
    !crc
}

#[path = "os/stand_in.rs"]
mod os;
use os::ignore_interrupts;
