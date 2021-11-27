use std::{
    collections::HashSet,
    ops::{Range, Sub},
    str::FromStr,
};

use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Game {
    board: Board,
    players: Vec<Player>,
    player_index: usize,
    bag: Bag,
}

#[derive(Serialize, Deserialize)]
pub struct Player(String);

impl Game {
    pub fn check_complete() {
        todo!()
    }
    // FIXME: allow up to two incorrect submissions before turn ends
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Board(Vec<Square>);

#[derive(Serialize, Deserialize, Debug)]
pub struct Bag(Vec<Tile>);

#[derive(Serialize, Deserialize, PartialEq, Copy, Clone)]
pub enum Tile {
    Char(char),
    Blank(Option<char>),
}

impl Default for Game {
    fn default() -> Self {
        Game {
            board: Board::standard().expect("standard board could not be built"),
            players: Default::default(),
            player_index: 0,
            bag: Bag::standard(),
        }
    }
}

pub static BOARD_SIZE: usize = 15;
static INDEX_OVERFLOW: usize = 15 * 15;

impl std::fmt::Debug for Tile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tile::Char(char) => write!(f, "[{}]", char),
            Tile::Blank(None) => write!(f, "[ ]"),
            Tile::Blank(Some(char)) => write!(f, "[({})]", char),
        }
    }
}

impl Tile {
    pub fn as_char(&self) -> Option<char> {
        match *self {
            Tile::Char(char) => Some(char),
            Tile::Blank(Some(char)) => Some(char),
            _ => None,
        }
    }
}

macro_rules! l {
    () => {
        Tile::Blank(None)
    };

    ($c:expr) => {
        Tile::Char($c)
    };
}

impl Bag {
    pub fn standard() -> Self {
        let counts = vec![
            (l!('A'), 9),
            (l!('B'), 2),
            (l!('C'), 2),
            (l!('D'), 4),
            (l!('E'), 12),
            (l!('F'), 2),
            (l!('G'), 3),
            (l!('H'), 2),
            (l!('I'), 9),
            (l!('J'), 1),
            (l!('K'), 1),
            (l!('L'), 4),
            (l!('M'), 2),
            (l!('N'), 6),
            (l!('O'), 8),
            (l!('P'), 2),
            (l!('Q'), 1),
            (l!('R'), 6),
            (l!('S'), 4),
            (l!('T'), 6),
            (l!('U'), 4),
            (l!('V'), 2),
            (l!('W'), 2),
            (l!('X'), 1),
            (l!('Y'), 2),
            (l!('Z'), 1),
            (l!(), 2),
        ];

        let mut bag = vec![];

        for (letter, count) in counts {
            for _ in 1..count {
                bag.push(letter);
            }
        }

        bag.shuffle(&mut thread_rng());

        Bag(bag)
    }
}

#[derive(Debug)]
pub enum Error {
    BoardParse(String),
}

impl Board {
    pub fn standard() -> Result<Self, Error> {
        let board_string = "
            3w .  .  2l .  .  .  3w .  .  .  2l .  .  3w 
            .  2w .  .  .  3l .  .  .  3l .  .  .  2w .   
            .  .  2w .  .  .  2l .  2l .  .  .  2w .  .   
            2l .  .  2w .  .  .  2l .  .  .  2w .  .  2l  
            .  .  .  .  2w .  .  .  .  .  2w .  .  .  .  
            .  3l .  .  .  3l .  .  .  3l .  .  .  3l .   
            .  .  2l .  .  .  2l .  2l .  .  .  2l .  .   
            3w .  .  2l .  .  .  2w .  .  .  2l .  .  3w  
            .  .  2l .  .  .  2l .  2l .  .  .  2l .  .   
            .  3l .  .  .  3l .  .  .  3l .  .  .  3l . 
            .  .  .  .  2w .  .  .  .  .  2w .  .  .  .   
            2l .  .  2w .  .  .  2l .  .  .  2w .  .  2l  
            .  .  2w .  .  .  2l .  2l .  .  .  2w .  .   
            .  2w .  .  .  3l .  .  .  3l .  .  .  2w .   
            3w .  .  2l .  .  .  3w .  .  .  2l .  .  3w
        ";

        Self::parse(board_string)
    }

    // FIXME: this doesn't parse a blank used as a letter, but maybe it doesn't need to
    // (that condition would only be used in a test; for persistence this should be serialized structurally.)
    pub fn parse(board_string: &str) -> Result<Self, Error> {
        let mut tiles = vec![];

        for token in board_string.split_whitespace() {
            match token {
                "." => tiles.push(Square::blank()),
                "3w" => tiles.push(Square::word_bonus(3)),
                "2w" => tiles.push(Square::word_bonus(2)),
                "3l" => tiles.push(Square::letter_bonus(3)),
                "2l" => tiles.push(Square::letter_bonus(2)),
                ref c => {
                    let parsed = char::from_str(c).map_err(|_| Error::BoardParse(c.to_string()))?;
                    let square = Square::Tile(Tile::Char(parsed));
                    tiles.push(square);
                }
            }
        }

        Ok(Self(tiles))
    }

    pub fn words(&self) -> impl Iterator<Item = Word> + '_ {
        let horizontal = Words::horizontal(self);
        let vertical = Words::vertical(self);
        horizontal.chain(vertical)
    }

    // FIXME: check dictionary and return Result instead
    fn new_words(&self, turn: &Turn) -> HashSet<Word> {
        let original: HashSet<Word> = self.words().collect();
        let overlay = Overlay { board: self, turn };
        let horizontal = Words::horizontal(&overlay);
        let vertical = Words::vertical(&overlay);
        let overlay_words: HashSet<Word> = horizontal.chain(vertical).collect();

        overlay_words.sub(&original)
    }

    fn score_turn(&self, turn: &Turn) -> Result<Vec<(String, usize)>, Error> {
        let mut scores = vec![];
        for word in self.new_words(turn) {
            scores.push((String::from(&word), self.score_word(&word)))
        }

        Ok(scores)
    }

    fn score_word(&self, word: &Word) -> usize {
        let word_bonus = self.word_bonus(&word.indexes);

        let mut score = 0;

        for (char, index) in word.char_indicies() {
            score += self.score_char(char, index);
        }

        score * word_bonus
    }

    fn get_square(&self, index: &usize) -> Option<&Square> {
        self.0.get(*index)
    }

    // FIXME: blank gets 0
    fn score_char(&self, char: char, index: &usize) -> usize {
        let letter_bonus = match self.get_square(index) {
            Some(Square::LetterBonus(multiplier)) => *multiplier,
            _ => 1,
        };

        score_char(&char) * letter_bonus
    }

    fn word_bonus(&self, indexes: &[usize]) -> usize {
        let mut bonus = 1;
        for index in indexes {
            if let Some(Square::WordBonus(multiplier)) = self.get_square(index) {
                bonus *= *multiplier
            }
        }

        bonus
    }

    fn commit_turn(&mut self, turn: &Turn) -> Result<(), Error> {
        // FIXME: ensure turn is valid, get scores
        for (index, char) in &turn.chars {
            let entry = &mut self.0[*index];
            let mut new_value = Square::Tile(Tile::Char(*char));
            std::mem::swap(entry, &mut new_value);
        }

        Ok(())
    }

    fn as_board_string(&self) -> String {
        let mut result = String::new();
        for (index, square) in self.0.iter().enumerate() {
            result.push_str(&format_square(square));
            if index % BOARD_SIZE == BOARD_SIZE - 1 {
                result.push('\n');
            }
        }

        result
    }
}

fn format_square(square: &Square) -> String {
    match square {
        Square::Blank => ".  ".to_string(),
        Square::Tile(tile) => match tile {
            Tile::Char(char) => format!("{}  ", char),
            Tile::Blank(Some(char)) => format!(":{} ", char),
            Tile::Blank(None) => ":: ".to_string(),
        },
        Square::LetterBonus(m) => format!("{}l ", m),
        Square::WordBonus(m) => format!("{}w ", m),
    }
}

fn score_char(char: &char) -> usize {
    match char {
        'A' => 1,
        'B' => 3,
        'C' => 3,
        'D' => 2,
        'E' => 1,
        'F' => 4,
        'G' => 2,
        'H' => 4,
        'I' => 1,
        'J' => 8,
        'K' => 5,
        'L' => 1,
        'M' => 3,
        'N' => 1,
        'O' => 1,
        'P' => 3,
        'Q' => 10,
        'R' => 1,
        'S' => 1,
        'T' => 1,
        'U' => 1,
        'V' => 4,
        'W' => 4,
        'X' => 8,
        'Y' => 4,
        'Z' => 10,
        _ => 0,
    }
}

trait GetChar {
    fn get_char(&self, index: usize) -> Option<char>;
}

struct Overlay<'a> {
    board: &'a Board,
    turn: &'a Turn,
}

impl GetChar for Board {
    fn get_char(&self, index: usize) -> Option<char> {
        self.0.get(index).and_then(|square| square.get_char())
    }
}

impl GetChar for Overlay<'_> {
    fn get_char(&self, index: usize) -> Option<char> {
        self.board
            .get_char(index)
            .or_else(|| self.turn.get_char(index))
    }
}

impl GetChar for Turn {
    fn get_char(&self, index: usize) -> Option<char> {
        self.chars
            .iter()
            .filter(|(i, _)| *i == index)
            .map(|(_, c)| *c)
            .next()
    }
}

impl From<Word> for String {
    fn from(word: Word) -> Self {
        word.string
    }
}

impl From<&Word> for String {
    fn from(word: &Word) -> Self {
        word.string.clone()
    }
}

#[derive(Debug)]
enum Direction {
    Horizontal,
    Vertical,
}

pub struct Words<'a, S> {
    cursor: usize,
    index: usize,
    direction: Direction,
    source: &'a S,
}

impl<S: GetChar> Words<'_, S> {
    fn horizontal(source: &S) -> Words<'_, S> {
        Words {
            cursor: 0,
            index: 0,
            direction: Direction::Horizontal,
            source,
        }
    }

    fn vertical(source: &S) -> Words<'_, S> {
        Words {
            cursor: 0,
            index: 0,
            direction: Direction::Vertical,
            source,
        }
    }

    fn advance(&mut self) {
        self.cursor += 1;
        self.index = transpose_index(self.cursor, &self.direction);
    }
}

impl<S: GetChar> Iterator for Words<'_, S> {
    type Item = Word;

    fn next(&mut self) -> Option<Self::Item> {
        let mut current = Word::new();

        // advance to next non-empty square
        loop {
            while self.source.get_char(self.index).is_none() {
                self.advance();

                if self.cursor >= INDEX_OVERFLOW {
                    return None;
                }
            }

            while let Some(char) = self.source.get_char(self.index) {
                current.push(self.index, char);
                println!(
                    "current = {:?} (cursor={} ({}))",
                    current, self.cursor, self.index
                );
                self.advance();

                // end of row
                if self.cursor % BOARD_SIZE == 0 {
                    break;
                }
            }

            if current.len() > 1 {
                return Some(current.clone());
            } else {
                self.advance();
                current.clear();
                println!("{:?} not long enough", current);
            }
        }
    }
}

// Word uniqueness is based on the indexes played, not the word itself (allow for duplicates)
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Word {
    indexes: Vec<usize>,
    string: String,
}

impl Word {
    pub fn new() -> Self {
        Word {
            indexes: Vec::new(),
            string: String::new(),
        }
    }

    pub fn push(&mut self, index: usize, char: char) {
        self.indexes.push(index);
        self.string.push(char);
    }

    pub fn clear(&mut self) {
        self.indexes.clear();
        self.string.clear();
    }

    pub fn len(&self) -> usize {
        self.indexes.len()
    }

    pub fn char_indicies(&self) -> impl Iterator<Item = (char, &usize)> {
        self.string.chars().zip(self.indexes.iter())
    }
}

fn transpose_index(index: usize, direction: &Direction) -> usize {
    match direction {
        Direction::Vertical => (index / BOARD_SIZE) + (index % BOARD_SIZE * BOARD_SIZE),
        Direction::Horizontal => index,
    }
}

pub struct Turn {
    chars: Vec<(usize, char)>,
    // map of indexes to letters
    // player
    // validations:
    // - player has the letters played
    // - new words are all valid
    // - new words all touch the existing words
    // - single direction of play

    // score!
}

pub struct TurnScore {
    words: Vec<(String, usize)>,
}

impl Turn {
    fn indexes(&self) -> impl Iterator<Item = &usize> {
        self.chars.iter().map(|(i, _)| i)
    }

    fn validate_unique_indexes(&self) -> bool {
        // all indexes should be unique
        todo!()
    }

    fn validate_linear(&self) -> bool {
        self.indexes()
            .map(|i| i % BOARD_SIZE)
            .collect::<HashSet<usize>>()
            .len()
            == 1
            || self
                .indexes()
                .map(|i| i / BOARD_SIZE)
                .collect::<HashSet<usize>>()
                .len()
                == 1
    }
}

// 0  1  2
// 3  4  5
// 6  7  8

// 0 1 2 3 4 5 6 7 8
// 0 3 6 1 4 7 2 5 8

// 0 * 3 % 16

#[derive(Debug, Serialize, Deserialize)]
enum Square {
    Blank,
    Tile(Tile),
    LetterBonus(usize),
    WordBonus(usize),
}

impl Square {
    fn blank() -> Self {
        Square::Blank
    }

    fn word_bonus(multiplier: usize) -> Self {
        Square::WordBonus(multiplier)
    }

    fn letter_bonus(multiplier: usize) -> Self {
        Square::LetterBonus(multiplier)
    }

    fn tile(&self) -> Option<&Tile> {
        if let Square::Tile(tile) = self {
            return Some(tile);
        }

        None
    }

    fn get_char(&self) -> Option<char> {
        self.tile().and_then(|tile| match tile {
            Tile::Char(char) => Some(*char),
            Tile::Blank(None) => None,
            Tile::Blank(Some(char)) => Some(*char),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_game_play_word() {}
    fn test_game_score_word() {}

    fn test_board_a() -> &'static str {
        "
            3w .  .  2l .  .  .  3w .  .  .  2l .  H  I 
            .  2w .  .  .  3l .  .  .  3l .  .  .  2w .   
            .  .  2w .  .  .  2l .  2l .  .  .  2w .  .   
            2l .  .  2w .  .  .  2l .  .  .  2w .  .  2l  
            .  .  .  .  2w .  .  .  .  .  2w .  .  .  .  
            .  3l .  .  .  3l .  .  .  3l .  .  .  3l .   
            .  .  2l .  .  .  2l .  2l .  .  .  2l .  .   
            3w .  .  2l .  .  .  A  M  P  L  E  .  .  3w  
            .  .  2l .  .  .  2l .  A  A  .  .  2l .  .   
            .  3l .  .  .  H  A  P  P  Y  .  .  .  3l . 
            .  .  .  .  2w .  .  .  .  E  2w .  .  .  .   
            2l .  .  2w .  .  .  2l .  R  .  2w .  .  O  
            .  .  2w .  .  .  2l .  2l .  .  .  2w .  O   
            .  2w .  .  .  3l .  .  .  3l .  .  .  2w Z   
            3w .  .  2l .  .  .  3w .  .  .  2l .  .  E
        "
    }

    #[test]
    fn test_board_words() {
        let board = Board::parse(test_board_a()).unwrap();
        let words: Vec<String> = board.words().map(Into::into).collect();
        let expected: Vec<String> = ["HI", "AMPLE", "AA", "HAPPY", "MAP", "PAYER", "OOZE"]
            .into_iter()
            .map(Into::into)
            .collect();

        assert_eq!(expected, words);
    }

    #[test]
    fn test_board_new_words() {
        let board = Board::parse(test_board_a()).unwrap();
        let turn = Turn {
            chars: vec![(111, 'S'), (126, 'L'), (156, 'T')],
        };

        assert!(turn.validate_linear());
        // let turn_score = board.score_turn(&turn);

        let new_words: HashSet<String> = board.new_words(&turn).iter().map(Into::into).collect();
        let expected: HashSet<String> = ["SAMPLE", "SLAT"].into_iter().map(Into::into).collect();

        assert_eq!(new_words, expected);
    }

    #[test]
    fn test_board_score_turn() {
        let board = Board::parse(test_board_a()).unwrap();
        let turn = Turn {
            chars: vec![(111, 'S'), (126, 'L'), (156, 'T')],
        };

        let scores: HashSet<(String, usize)> =
            board.score_turn(&turn).unwrap().into_iter().collect();

        assert_eq!(
            scores,
            [("SLAT".to_owned(), 5), ("SAMPLE".to_owned(), 10)]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn test_board_commit_turn() {
        let mut board = Board::parse(test_board_a()).unwrap();
        let turn = Turn {
            chars: vec![(111, 'S'), (126, 'L'), (156, 'T')],
        };

        board.commit_turn(&turn).unwrap();

        println!("{:?}", board);
        println!("{}", board.as_board_string());
        let words: Vec<String> = board.words().map(Into::into).collect();

        let expected: Vec<String> = [
            "HI", "SAMPLE", "AA", "HAPPY", "SLAT", "MAP", "PAYER", "OOZE",
        ]
        .into_iter()
        .map(Into::into)
        .collect();

        assert_eq!(words, expected);
    }

    #[test]
    fn test_game_play() {
        let game = Game::default();
    }
}
