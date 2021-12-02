use std::{
    collections::HashSet,
    ops::{Range, Sub},
    str::FromStr,
};

use rand::{prelude::StdRng, thread_rng, SeedableRng};
use rand::{seq::SliceRandom, Rng};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Game {
    board: Board,
    players: Vec<Player>,
    player_index: usize,
    bag: Bag,
    racks: Vec<Rack>,
    scores: Vec<Vec<TurnScore>>,
    state: State,
}

#[derive(Debug, Serialize, Deserialize)]
enum State {
    Pre,
    Started,
    Over,
}

impl Default for State {
    fn default() -> Self {
        Self::Pre
    }
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("players", &self.players)
            .field("racks", &self.racks)
            .field("player_index", &self.player_index)
            .field("bag", &self.bag)
            .field("scores", &self.scores)
            .field("state", &self.state)
            .finish()?;

        f.write_str("\n")?;
        f.write_str(&self.board.as_board_string())
    }
}

// FIXME: this should also have a uuid
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct Player(pub(crate) String);

pub type Rack = Vec<Tile>;

impl Game {
    pub fn check_complete() {
        todo!()
    }

    pub fn start(&mut self) -> Result<(), Error> {
        self.init_racks()?;
        self.init_player_index()?;
        self.init_scores()?;
        self.state = State::Started;
        Ok(())
    }

    pub fn is_over(&self) -> bool {
        matches!(self.state, State::Over)
    }

    fn init_scores(&mut self) -> Result<(), Error> {
        for _ in &self.players {
            self.scores.push(Vec::new())
        }
        Ok(())
    }

    // FIXME ensure players are unique
    pub fn add_player(&mut self, player: Player) -> Result<(), Error> {
        self.players.push(player);
        Ok(())
    }

    fn init_racks(&mut self) -> Result<(), Error> {
        for index in 0..self.players.len() {
            self.racks.push(Rack::default());
            self.fill_rack_at(index)?;
        }

        Ok(())
    }

    fn fill_rack_at(&mut self, index: usize) -> Result<(), Error> {
        let rack = &mut self.racks[index];

        while rack.len() < 7 {
            match self.bag.pop() {
                None => {
                    return Ok(());
                }
                Some(tile) => rack.push(tile),
            }
        }

        Ok(())
    }

    fn init_player_index(&mut self) -> Result<(), Error> {
        self.player_index = thread_rng().gen_range(0..self.players.len());
        Ok(())
    }

    pub fn play(&mut self, turn: Turn) -> Result<(), Error> {
        match self.state {
            State::Pre => return Err(Error::NotStarted),
            State::Over => return Err(Error::GameOver),
            _ => (),
        }
        // FIXME: make this an atomic operation? Need something like immutable data;
        // the validation should otherwise check everything
        self.validate_turn(&turn)?;
        self.score_turn(&turn)?;
        self.spend_tiles(&turn)?;
        self.board.commit_turn(&turn)?;
        self.fill_rack_at(self.player_index)?;
        self.next_player()?;
        self.check_game_over()?;
        Ok(())
    }

    fn check_game_over(&mut self) -> Result<(), Error> {
        if self.bag.0.is_empty() && self.racks.iter().any(|r| r.is_empty()) {
            self.state = State::Over;

            for (index, rack) in self.racks.iter().enumerate() {
                let remaining = rack.iter().fold(0, |sum, tile| sum + score_tile(tile));
                if remaining > 0 {
                    self.scores[index].push(TurnScore {
                        scores: vec![("(remaining tiles)".to_string(), -remaining)],
                    })
                }
            }
        }

        // FIXME: check consecutive passes

        Ok(())
    }

    pub fn swap(&mut self, turn: Turn) -> Result<(), Error> {
        todo!()
    }

    pub fn pass(&mut self) -> Result<(), Error> {
        if self.bag.0.len() > 6 {
            return Err(Error::CannotPass);
        }

        // fixme
        todo!();

        Ok(())
    }

    fn validate_turn(&mut self, turn: &Turn) -> Result<(), Error> {
        turn.validate()?;
        // FIXME: any way to do this once? This clone currently happens again in the commit.
        Self::spend_tiles_inner(turn, self.racks[self.player_index].clone())?;
        // FIXME: validate connected to other words, or on space 112 (initial turn)
        // FIXME: validate that turn indexes aren't already occupied
        // FIXME: validate words in dictionary
        Ok(())
    }

    fn score_turn(&mut self, turn: &Turn) -> Result<(), Error> {
        let overlay = Overlay {
            board: &self.board,
            turn,
        };
        let score = overlay.score()?;
        self.scores[self.player_index].push(score);

        Ok(())
    }

    // advance cursor to next player
    fn next_player(&mut self) -> Result<(), Error> {
        self.player_index += 1;
        self.player_index %= self.players.len();
        Ok(())
    }

    fn spend_tiles(&mut self, turn: &Turn) -> Result<(), Error> {
        let mut new_rack = Self::spend_tiles_inner(turn, self.racks[self.player_index].clone())?;
        std::mem::swap(&mut self.racks[self.player_index], &mut new_rack);

        Ok(())
    }

    fn spend_tiles_inner(turn: &Turn, mut rack: Rack) -> Result<Rack, Error> {
        for (_, tile) in &turn.tiles {
            // FIXME: handle blanks
            let index = rack
                .iter()
                .position(|rack_tile| match tile {
                    Tile::Char(..) => tile == rack_tile,
                    Tile::Blank(Some(char)) => matches!(rack_tile, Tile::Blank(None)),
                    Tile::Blank(None) => false,
                })
                .ok_or_else(|| Error::NoTileToSpend(*tile))?;
            rack.remove(index);
        }
        Ok(rack)
    }
    // FIXME: allow up to two incorrect submissions before turn ends
}

impl From<&str> for Player {
    fn from(name: &str) -> Self {
        Player(name.to_owned())
    }
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
            racks: Default::default(),
            scores: Default::default(),
            state: Default::default(),
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
    pub fn pop(&mut self) -> Option<Tile> {
        self.0.pop()
    }

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

#[derive(Debug, PartialEq)]
pub enum Error {
    BoardParse(String),
    NoTileToSpend(Tile),
    TurnIndexesNotUnique,
    TurnNotLinear,
    NotStarted,
    GameOver,
    BlankTileInTurn,
    CannotPass,
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
    fn new_words(&self, turn: &Turn) -> Vec<Word> {
        let overlay = Overlay { board: self, turn };
        overlay.new_words()
    }

    fn get_square(&self, index: &usize) -> Option<&Square> {
        self.0.get(*index)
    }

    fn word_bonus(&self, indexes: &[usize]) -> isize {
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
        for (index, tile) in &turn.tiles {
            let entry = &mut self.0[*index];
            let mut new_value = Square::Tile(*tile);
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

    fn get_tile(&self, index: &usize) -> Option<&Tile> {
        match self.0.get(*index).expect("index out of bounds") {
            Square::Blank => None,
            Square::Tile(tile) => Some(tile),
            Square::LetterBonus(_) => None,
            Square::WordBonus(_) => None,
        }
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

fn score_tile(tile: &Tile) -> isize {
    match tile {
        Tile::Char(c) => score_char(c),
        Tile::Blank(_) => 0,
    }
}

fn score_char(char: &char) -> isize {
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

pub trait GetChar {
    fn get_char(&self, index: usize) -> Option<char>;
}

struct Overlay<'a> {
    board: &'a Board,
    turn: &'a Turn,
}

impl Overlay<'_> {
    fn new_words(&self) -> Vec<Word> {
        let original: Vec<Word> = self.board.words().collect();
        let horizontal = Words::horizontal(self);
        let vertical = Words::vertical(self);
        let mut overlay_words: Vec<Word> = horizontal.chain(vertical).collect();

        for word in original {
            overlay_words.retain(|w| *w != word);
        }

        overlay_words
    }

    // FIXME: blank gets 0
    fn score_tile(&self, tile: &Tile, index: &usize) -> isize {
        score_tile(tile) * self.letter_bonus(index)
    }

    fn get_tile(&self, index: &usize) -> Option<&Tile> {
        self.turn
            .get_tile(index)
            .or_else(|| self.board.get_tile(index))
    }

    fn score_word(&self, word: &Word) -> isize {
        let word_bonus = self.word_bonus(&word.indexes);

        let mut score = 0;

        for index in &word.indexes {
            let tile = self
                .get_tile(index)
                .expect("tile unexpectedly missing from word");
            score += self.score_tile(tile, index);
        }

        score * word_bonus
    }

    fn word_bonus(&self, indexes: &[usize]) -> isize {
        self.board.word_bonus(indexes)
    }

    fn letter_bonus(&self, index: &usize) -> isize {
        match self.board.get_square(index) {
            Some(Square::LetterBonus(multiplier)) => *multiplier,
            _ => 1,
        }
    }

    pub fn score(&self) -> Result<TurnScore, Error> {
        let mut scores = vec![];
        for word in self.new_words() {
            scores.push((String::from(&word), self.score_word(&word)))
        }

        Ok(TurnScore { scores })
    }
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
        self.tiles
            .iter()
            .filter(|(i, _)| *i == index)
            .map(|(_, tile)| tile.as_char())
            .next()
            .unwrap_or(None)
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
    tiles: Vec<(usize, Tile)>,
    // map of indexes to letters
    // player
    // validations:
    // - player has the letters played
    // - new words are all valid
    // - new words all touch the existing words
    // - single direction of play

    // score!
}

#[derive(Deserialize, Serialize, Debug, Eq, PartialEq)]
pub struct TurnScore {
    scores: Vec<(String, isize)>,
}

impl Turn {
    fn indexes(&self) -> impl Iterator<Item = &usize> {
        self.tiles.iter().map(|(i, _)| i)
    }

    fn validate(&self) -> Result<(), Error> {
        self.validate_unique_indexes()?;
        self.validate_linear()?;

        Ok(())
    }

    fn validate_unique_indexes(&self) -> Result<(), Error> {
        // all indexes should be unique
        if self.indexes().count() == self.indexes().collect::<HashSet<&usize>>().len() {
            Ok(())
        } else {
            Err(Error::TurnIndexesNotUnique)
        }
    }

    fn validate_linear(&self) -> Result<(), Error> {
        if self
            .indexes()
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
        {
            Ok(())
        } else {
            Err(Error::TurnNotLinear)
        }
    }

    fn get_tile(&self, index: &usize) -> Option<&Tile> {
        self.tiles
            .iter()
            .filter(|(i, _)| i == index)
            .map(|(_, tile)| tile)
            .next()
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
    LetterBonus(isize),
    WordBonus(isize),
}

impl Square {
    fn blank() -> Self {
        Square::Blank
    }

    fn word_bonus(multiplier: isize) -> Self {
        Square::WordBonus(multiplier)
    }

    fn letter_bonus(multiplier: isize) -> Self {
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
            tiles: vec![(111, l!('S')), (126, l!('L')), (156, l!('T'))],
        };

        assert!(turn.validate_linear().is_ok());
        // let turn_score = board.score_turn(&turn);

        let new_words: HashSet<String> = board.new_words(&turn).iter().map(Into::into).collect();
        let expected: HashSet<String> = ["SAMPLE", "SLAT"].into_iter().map(Into::into).collect();

        assert_eq!(new_words, expected);
    }

    #[test]
    fn test_overlay_score_turn() {
        let board = Board::parse(test_board_a()).unwrap();
        let turn = Turn {
            tiles: vec![(111, l!('S')), (126, l!('L')), (156, l!('T'))],
        };

        let overlay = Overlay {
            board: &board,
            turn: &turn,
        };

        let scores: HashSet<(String, isize)> =
            overlay.score().unwrap().scores.into_iter().collect();

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
            tiles: vec![(111, l!('S')), (126, l!('L')), (156, l!('T'))],
        };

        board.commit_turn(&turn).unwrap();

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
    fn test_game_init() {
        let mut game = Game::default();
        game.add_player(Player::from("Frankie")).unwrap();
        game.add_player(Player::from("Ada")).unwrap();
        game.start().unwrap();

        assert_eq!(game.racks.len(), 2);
        assert_eq!(game.racks[0].len(), 7);
        assert_eq!(game.racks[1].len(), 7);
    }

    fn test_bag() -> Bag {
        let bag = vec![
            l!('Q'),
            l!('A'),
            l!('P'),
            l!('S'),
            l!('T'),
            l!('I'),
            l!('E'),
            l!('X'),
            l!('L'),
            l!('I'),
            l!('T'),
            l!('R'),
            l!('A'),
            l!('M'),
            l!('S'),
        ];

        Bag(bag)
    }

    #[test]
    fn test_game_play() {
        let mut game = Game::default();
        game.bag = test_bag();
        game.add_player(Player::from("Frankie")).unwrap();
        game.add_player(Player::from("Ada")).unwrap();

        game.start().unwrap();
        game.player_index = 0;

        assert_eq!(game.racks.len(), 2);
        assert_eq!(game.racks[0].len(), 7);
        assert_eq!(game.racks[1].len(), 7);

        println!("{:#?}", game);

        let turn_a = Turn {
            tiles: vec![(112, l!('M')), (113, l!('A')), (114, l!('R'))],
        };
        game.play(turn_a).unwrap();
        println!("{:#?}", game);

        assert_eq!(
            game.racks[0],
            vec![l!('S'), l!('T'), l!('I'), l!('L'), l!('Q')]
        );

        let words: Vec<String> = game.board.words().map(Into::into).collect();
        assert_eq!(game.player_index, 1);
        assert_eq!(words, vec!["MAR".to_string()]);

        assert_eq!(
            game.scores[0],
            vec![TurnScore {
                scores: vec![("MAR".to_owned(), 10)]
            }]
        );

        let turn_b = Turn {
            tiles: vec![(126, l!('T')), (127, l!('A')), (128, l!('X'))],
        };

        game.play(turn_b).unwrap();
        println!("{:#?}", game);

        assert_eq!(game.racks[1], vec![l!('E'), l!('I'), l!('S'), l!('P')]);

        assert_eq!(
            game.scores[1],
            vec![TurnScore {
                scores: vec![
                    ("TAX".to_string(), 19),
                    ("MA".to_string(), 4),
                    ("AX".to_string(), 17),
                ]
            }]
        );
        assert_eq!(game.player_index, 0);

        let turn_c_err_1 = Turn {
            tiles: vec![(140, l!('T')), (127, l!('A')), (128, l!('X'))],
        };

        assert_eq!(game.play(turn_c_err_1).unwrap_err(), Error::TurnNotLinear);

        let turn_c_err_2 = Turn {
            tiles: vec![(140, l!('T')), (141, l!('A')), (142, l!('X'))],
        };

        assert_eq!(
            game.play(turn_c_err_2).unwrap_err(),
            Error::NoTileToSpend(l!('A'))
        );

        let turn_c_1 = Turn {
            tiles: vec![(141, l!('I')), (156, l!('L'))],
        };

        game.play(turn_c_1).unwrap();
        println!("{:#?}", game);

        assert_eq!(
            game.scores[0],
            vec![
                TurnScore {
                    scores: vec![("MAR".to_owned(), 10)]
                },
                TurnScore {
                    scores: vec![("TIL".to_owned(), 3)]
                }
            ]
        );

        let turn_d = Turn {
            tiles: vec![
                (169, l!('P')),
                (170, l!('I')),
                (171, l!('E')),
                (172, l!('S')),
            ],
        };

        game.play(turn_d).unwrap();
        println!("{:#?}", game);

        assert!(game.is_over());

        assert_eq!(
            game.scores[0],
            vec![
                TurnScore {
                    scores: vec![("MAR".to_owned(), 10)]
                },
                TurnScore {
                    scores: vec![("TIL".to_owned(), 3)]
                },
                TurnScore {
                    scores: vec![("(remaining tiles)".to_string(), -12)]
                }
            ]
        )
    }

    #[test]
    fn test_game_play_with_blanks() {
        let mut game = Game::default();
        let bag = Bag(vec![
            l!('Q'),
            l!('A'),
            l!('P'),
            l!('S'),
            l!('T'),
            l!('I'),
            l!('E'),
            l!('X'),
            l!('L'),
            l!('I'),
            l!('T'),
            l!('R'),
            l!('A'),
            l!(),
            l!('S'),
        ]);

        game.bag = bag;
        game.add_player(Player::from("Frankie")).unwrap();
        game.add_player(Player::from("Ada")).unwrap();

        game.start().unwrap();
        game.player_index = 0;

        assert_eq!(game.racks.len(), 2);
        assert_eq!(game.racks[0].len(), 7);
        assert_eq!(game.racks[1].len(), 7);

        let turn_a = Turn {
            tiles: vec![
                (111, l!('S')),
                (112, Tile::Blank(Some('M'))),
                (113, l!('A')),
                (114, l!('R')),
                (115, l!('T')),
            ],
        };

        game.play(turn_a).unwrap();
        println!("{:#?}", game);

        assert_eq!(
            game.scores[0],
            vec![TurnScore {
                scores: vec![("SMART".to_string(), 8)]
            }]
        );

        let words: Vec<String> = game.board.words().map(Into::into).collect();
        assert_eq!(words, vec!["SMART".to_string()]);

        let turn_b = Turn {
            tiles: vec![(127, l!('A')), (128, l!('X'))],
        };

        game.play(turn_b).unwrap();
        assert_eq!(
            game.scores[1],
            vec![TurnScore {
                scores: vec![
                    ("AX".to_string(), 17),
                    ("MA".to_string(), 1), // blank M doesn't count
                    ("AX".to_string(), 17)
                ]
            }],
        );
    }
}
