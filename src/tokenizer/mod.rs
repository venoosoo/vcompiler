use std::fmt;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TokenType {
    I32,
    I8,
    I16,
    I64,
    U8,
    U16,
    U32,
    U64,
    Var,
    CharValue,
    Num,
    HexNum,
    Eq,
    Add,
    Mul,
    Sub,
    Div,
    OpenParen,
    CloseParen,
    OpenScope,
    CloseScope,
    If,
    Else,
    AsertEq,
    NotEq,
    Not,
    Less,
    LessThan,
    More,
    MoreThan,
    And,
    Or,
    While,
    For,
    Inc,
    Dec,
    Void,
    Return,
    Coma,
    String,
    Struct,
    OpenBracket,
    Dot,
    CloseBracket,
    Remainder,
    Address,
    Access,
    Asm,
    Func,
    Colon,
    Import,
    Global,
    SizeOf,
    Enum,
    As,
    Match,
    Other,
    BitOr,
    BitXor,
    OctalNum,
    BitNot,
    Break,
    Continue,
    LeftShift,
    RightShift,
    ExternFn,
    Semi,
}
#[derive(Clone, Debug)]
pub struct Token {
    pub line: usize,
    pub col: usize,
    pub file: String,
    pub token: TokenType,
    pub value: Option<String>,
}

impl fmt::Display for Tokenizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, token) in self.m_res.iter().enumerate() {
            let _ = match &token.value {
                Some(v) => write!(
                    f,
                    "Token {}: type: {:?}, value: \"{}\", file: {:?}\n",
                    index, token.token, v, self.file
                ),
                None => write!(
                    f,
                    "Token {}: type: {:?}, value: None, file: {:?}\n",
                    index, token.token, self.file
                ),
            };
        }
        Ok(())
    }
}

pub struct Tokenizer {
    m_index: usize,
    m_src: Vec<char>,
    m_buf: String,
    pub line: usize,
    pub col: usize,
    pub file: String,
    pub m_res: Vec<Token>,
}

impl Tokenizer {
    pub fn new(file: String, file_path: String) -> Self {
        Tokenizer {
            m_index: 0,
            m_src: file.chars().collect(),
            m_buf: String::new(),
            file: file_path,
            line: 1,
            col: 0,
            m_res: Vec::new(),
        }
    }

    fn push_token(&mut self, token: TokenType, value: Option<String>) {
        let x = Token {
            token,
            value,
            line: self.line,
            col: self.col,
            file: self.file.clone(),
        };
        self.m_res.push(x);
    }

    fn peek(&self, offset: usize) -> char {
        let pos: usize = self.m_index + offset;
        if pos >= self.m_src.len() {
            panic!("Trying to peek more than m_src len");
        }
        self.m_src[pos]
    }

    fn consume(&mut self) -> char {
        if self.m_index >= self.m_src.len() {
            panic!("Trying to consume more than m_src len");
        }
        self.m_index += 1;
        self.m_src[self.m_index - 1]
    }

    pub fn tokenize(&mut self) {
        while self.m_index <= self.m_src.len() - 1 {
            self.col += 1;
            if self.peek(0) == '\n' {
                self.line += 1;
                self.col = 0;
            }
            if self.peek(0).is_alphabetic() {
                let v = self.consume();
                self.m_buf.push(v);
                while self.peek(0).is_alphanumeric() || self.peek(0) == '_' {
                    let v = self.consume();
                    self.m_buf.push(v);
                }
                match self.m_buf.as_str() {
                    "i32" => self.push_token(TokenType::I32, None),
                    "i16" => self.push_token(TokenType::I16, None),
                    "i64" => self.push_token(TokenType::I64, None),
                    "i8" => self.push_token(TokenType::I8, None),
                    "u32" => self.push_token(TokenType::U32, None),
                    "u16" => self.push_token(TokenType::U16, None),
                    "u64" => self.push_token(TokenType::U64, None),
                    "u8" => self.push_token(TokenType::U8, None),
                    "if" => self.push_token(TokenType::If, None),
                    "else" => self.push_token(TokenType::Else, None),
                    "and" => self.push_token(TokenType::And, None),
                    "or" => self.push_token(TokenType::Or, None),
                    "while" => self.push_token(TokenType::While, None),
                    "for" => self.push_token(TokenType::For, None),
                    "void" => self.push_token(TokenType::Void, None),
                    "return" => self.push_token(TokenType::Return, None),
                    "struct" => self.push_token(TokenType::Struct, None),
                    "asm" => self.push_token(TokenType::Asm, None),
                    "fn" => self.push_token(TokenType::Func, None),
                    "import" => self.push_token(TokenType::Import, None),
                    "global" => self.push_token(TokenType::Global, None),
                    "sizeof" => self.push_token(TokenType::SizeOf, None),
                    "break" => self.push_token(TokenType::Break, None),
                    "continue" => self.push_token(TokenType::Continue, None),
                    "enum" => self.push_token(TokenType::Enum, None),
                    "match" => self.push_token(TokenType::Match, None),
                    "as" => self.push_token(TokenType::As, None),
                    "extern" => self.push_token(TokenType::ExternFn, None),
                    // we think its variable
                    _ => self.push_token(TokenType::Var, Some(self.m_buf.clone())),
                }
                self.m_buf = "".to_string();
            } else if self.peek(0).is_numeric() {
                let v = self.consume();
                self.m_buf.push(v);

                if self.peek(0) == 'x' {
                    self.consume();
                    self.m_buf.clear();
                    while self.peek(0).is_numeric() {
                        let v = self.consume();
                        self.m_buf.push(v);
                    }
                    self.push_token(TokenType::HexNum, Some(self.m_buf.clone()));
                    self.m_buf = "".to_string();
                    continue;
                }

                if self.peek(0) == 'o' {
                    self.consume();
                    self.m_buf.clear();
                    while self.peek(0).is_numeric() {
                        let v = self.consume();
                        self.m_buf.push(v);
                    }
                    self.push_token(TokenType::OctalNum, Some(self.m_buf.clone()));
                    self.m_buf = "".to_string();
                    continue;
                }

                while self.peek(0).is_numeric() {
                    let v = self.consume();
                    self.m_buf.push(v);
                }
                self.push_token(TokenType::Num, Some(self.m_buf.clone()));
                self.m_buf = "".to_string();
            } else {
                let smth = self.consume();
                match smth {
                    '|' => self.push_token(TokenType::BitOr, None),
                    '^' => self.push_token(TokenType::BitXor, None),
                    '~' => self.push_token(TokenType::BitNot, None),

                    ':' => self.push_token(TokenType::Colon, None),
                    '%' => self.push_token(TokenType::Remainder, None),
                    '\'' => {
                        let character = self.consume();
                        if character.is_ascii() {
                            self.push_token(
                                TokenType::CharValue,
                                Some((character as u8).to_string()),
                            );
                        } else {
                            panic!("trying to get ascii of unkown value");
                        }
                        self.consume();
                    }
                    '.' => {
                        self.push_token(TokenType::Dot, None);
                    }
                    '=' => {
                        if self.peek(0) == '=' {
                            self.push_token(TokenType::AsertEq, None);
                            self.consume();
                        } else {
                            self.push_token(TokenType::Eq, None);
                        }
                    }
                    ';' => self.push_token(TokenType::Semi, None),
                    '+' => {
                        if self.peek(0) == '+' {
                            self.push_token(TokenType::Inc, Some("++".to_string()));
                            self.consume();
                        } else {
                            self.push_token(TokenType::Add, Some('+'.to_string()))
                        }
                    }
                    '-' => {
                        if self.peek(0) == '>' {
                            self.consume();
                            self.push_token(TokenType::Access, None);
                            continue;
                        }
                        if self.peek(0) == '-' {
                            self.push_token(TokenType::Dec, Some("--".to_string()));
                            self.consume();
                        } else {
                            self.push_token(TokenType::Sub, Some('-'.to_string()))
                        }
                    }
                    '&' => self.push_token(TokenType::Address, None),
                    '*' => self.push_token(TokenType::Mul, Some('*'.to_string())),
                    '/' => {
                        if self.peek(0) == '/' {
                            let current_line = self.line;
                            while current_line == self.line {
                                if self.consume() == '\n' {
                                    self.line += 1;
                                    self.col = 0;
                                }
                            }
                        } else {
                            self.push_token(TokenType::Div, Some('/'.to_string()))
                        }
                    }
                    '(' => self.push_token(TokenType::OpenParen, Some('('.to_string())),
                    ')' => self.push_token(TokenType::CloseParen, Some(')'.to_string())),
                    '{' => self.push_token(TokenType::OpenScope, None),
                    '}' => self.push_token(TokenType::CloseScope, None),
                    '[' => self.push_token(TokenType::OpenBracket, None),
                    ']' => self.push_token(TokenType::CloseBracket, None),
                    '_' => self.push_token(TokenType::Other, Some('_'.to_string())),
                    '<' => {
                        if self.peek(0) == '=' {
                            self.push_token(TokenType::LessThan, None);
                            self.consume();
                        } else if self.peek(0) == '<' {
                            self.push_token(TokenType::LeftShift, None);
                            self.consume();
                        } else {
                            self.push_token(TokenType::Less, None);
                        }
                    }
                    '>' => {
                        if self.peek(0) == '=' {
                            self.push_token(TokenType::MoreThan, None);
                            self.consume();
                        } else if self.peek(0) == '>' {
                            self.push_token(TokenType::RightShift, None);
                            self.consume();
                        } else {
                            self.push_token(TokenType::More, None);
                        }
                    }
                    '!' => {
                        if self.peek(0) == '=' {
                            self.push_token(TokenType::NotEq, None);
                            self.consume();
                        } else {
                            self.push_token(TokenType::Not, None);
                        }
                    }
                    ',' => self.push_token(TokenType::Coma, Some(",".to_string())),
                    '"' => {
                        while self.peek(0) != '"' {
                            let v = self.consume();
                            self.m_buf.push(v);
                        }
                        self.consume();
                        self.push_token(TokenType::String, Some(self.m_buf.clone()));
                    }

                    _ => {}
                }
                self.m_buf = "".to_string();
            }
        }
    }
}
