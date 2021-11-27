mod scrabble;

fn main() {
    let bag = scrabble::Bag::standard();
    let board = scrabble::Board::standard();

    println!("{:?}, {:?}", bag, board);
}
