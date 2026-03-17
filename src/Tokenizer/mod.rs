use std::fmt;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TokenType {
    IntType,
    CharType,
    ShortType,
    LongType,
    Var,
    CharValue,
    Num,
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
    Semi,
}
#[derive(Clone, Debug)]
pub struct Token {
    pub token: TokenType,
    pub value: Option<String>,
}

impl fmt::Display for Tokenizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, token) in self.m_res.iter().enumerate() {
            let _ = match &token.value {
                Some(v) => write!(
                    f,
                    "Token {}: type: {:?}, value: \"{}\"\n",
                    index, token.token, v
                ),
                None => write!(f, "Token {}: type: {:?}, value: None\n", index, token.token),
            };
        }
        Ok(())
    }
}

pub struct Tokenizer {
    m_index: usize,
    m_src: Vec<char>,
    m_buf: String,
    pub m_res: Vec<Token>,
}

impl Tokenizer {
    pub fn new(file: String) -> Self {
        Tokenizer {
            m_index: 0,
            m_src: file.chars().collect(),
            m_buf: String::new(),
            m_res: Vec::new(),
        }
    }

    fn push_token(&mut self, token: TokenType, value: Option<String>) {
        let x = Token { token, value };
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
            if self.peek(0).is_alphabetic() {
                let v = self.consume();
                self.m_buf.push(v);
                while self.peek(0).is_alphanumeric() || self.peek(0) == '_' {
                    let v = self.consume();
                    self.m_buf.push(v);
                }
                match self.m_buf.as_str() {
                    "int" => self.push_token(TokenType::IntType, None),
                    "short" => self.push_token(TokenType::ShortType, None),
                    "long" => self.push_token(TokenType::LongType, None),
                    "char" => self.push_token(TokenType::CharType, None),
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
                    // we think its variable
                    _ => self.push_token(TokenType::Var, Some(self.m_buf.clone())),
                }
                self.m_buf = "".to_string();
            } else if self.peek(0).is_numeric() {
                let v = self.consume();
                self.m_buf.push(v);
                while self.peek(0).is_numeric() {
                    let v = self.consume();
                    self.m_buf.push(v);
                }
                self.push_token(TokenType::Num, Some(self.m_buf.clone()));
                self.m_buf = "".to_string();
            } else {
                let smth = self.consume();
                match smth {
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
                    '/' => self.push_token(TokenType::Div, Some('/'.to_string())),
                    '(' => self.push_token(TokenType::OpenParen, Some('('.to_string())),
                    ')' => self.push_token(TokenType::CloseParen, Some(')'.to_string())),
                    '{' => self.push_token(TokenType::OpenScope, None),
                    '}' => self.push_token(TokenType::CloseScope, None),
                    '[' => self.push_token(TokenType::OpenBracket, None),
                    ']' => self.push_token(TokenType::CloseBracket, None),
                    '<' => {
                        if self.peek(0) == '=' {
                            self.push_token(TokenType::LessThan, None);
                            self.consume();
                        } else {
                            self.push_token(TokenType::Less, None);
                        }
                    }
                    '>' => {
                        if self.peek(0) == '=' {
                            self.push_token(TokenType::MoreThan, None);
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
