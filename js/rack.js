import { select } from 'd3-selection'

class Rack {
  constructor(owner, el, letters, size) {
    if (size === undefined) {
      size = 10;
    }
    this.element = select(el)
    this.squares = new Array(size);
    this.letters = letters;
    this._cursor = undefined;
    this.size = size

    this.owner = owner
  }

  unfocus() {
    this.cursor = undefined;
  }

  hasFocus() {
    return (this.cursor !== undefined);
  }

  handleKeydown(event) {
    switch (event.key) {
      case "ArrowLeft": {
        event.preventDefault();
        return this._moveCursorLeft();
      }
      case "ArrowRight": {
        event.preventDefault();
        return this._moveCursorRight();
      }
    }

    let swapIndex;
    let moveFn = this._moveCursorRight;
    if (event.which === 8) {
      swapIndex = this._lastEmptyIndex();
      moveFn = this._moveCursorLeft;
    }

    if (event.which >= 65 && event.which <= 90) {
      swapIndex = this._searchLetter(String.fromCharCode(event.which));
    }

    if (swapIndex !== undefined) {
      let current = this.squares[this.cursor];
      let swap = this.squares[swapIndex];

      this.squares[this.cursor] = swap;
      this.squares[swapIndex] = current;
      moveFn.apply(this);
      this.draw();
    }
  }

  searchUnusedLetter(char) {
    for (let i = 0; i < this.squares.length; i++) {
      if (this.squares[i] && !this.squares[i].used && this.squares[i].char == char) {
        this.squares[i].used = true;
        this.draw();
        return char;
      }
    }

    if (char !== "BLANK") {
      return this.searchUnusedLetter("BLANK");
    }

    return false // letter not found
  }

  replaceUsedLetter(char, draw) {
    if (char[0] === ":") {
      return this.replaceUsedLetter("BLANK", draw);
    }

    if (draw === undefined) { draw = true }

    for (let i = (this.size - 1); i >= 0; i--) {
      if (this.squares[i] && this.squares[i].used && this.squares[i].char == char) {
        this.squares[i].used = false;
        if (draw) { this.draw() }
        return true;
      }
    }

    return false;
  }

  _searchLetter(char) {
    if (this.cursor) {
      let searchSegment = this.squares.slice(this.cursor);
      for (let i = 0; i < searchSegment.length; i++) {
        if (searchSegment[i] && searchSegment[i].char == char) {
          return this.cursor + i;
        }
      }
    }

    // otherwise search the rest
    for (let i = 0; i < this.squares.length; i++) {
      if (this.squares[i] && this.squares[i].char == char) {
        return i;
      }
    }
  }

  _moveCursorLeft() {
    if (this.cursor !== undefined) {
      this.cursor = Math.max(this.cursor - 1, 0);
    }
  }

  _moveCursorRight() {
    if (this.cursor !== undefined) {
      this.cursor = Math.min(this.cursor + 1, (this.size - 1));
    }
  }

  // FIXME: rename
  set letters(newLetters) {
    let searchIndices = {};

    // clear anything still used
    for (let i = 0; i < this.size; i++) {
      if (this.squares[i] && this.squares[i].used) { this.squares[i] = undefined }
    }

    // - mark all current letters 'used'
    for (let i = 0; i < this.size; i++) {
      if (this.squares[i]) {
        this.squares[i].used = true
      }
    }

    let toInsert = [];

    newLetters.forEach(e => {
      if (!this.replaceUsedLetter(e, false)) {
        toInsert.push(e)
      }
    }); // false - don't draw yet

    // sweep remaining used letters -- this can arise if a player
    // has two windows open simultaneously
    for (let i = 0; i < this.size; i++) {
      if (this.squares[i] && this.squares[i].used) { this.squares[i] = undefined }
    }

    toInsert.forEach(char => {
      this.squares[this._lastEmptyIndex()] = { char: char };
    });


    this.draw();
  }

  draw() {
    let container = this.element;
    let squares = container.selectAll('div.rack-square').data(this.squares);

    let enterJoin = squares
      .enter()
      .append('div')
      .attr('class', 'rack-square tile')

    let currentSquares = squares.merge(enterJoin);
    currentSquares.html(d => d && (d.char === "BLANK" ? "" : d.char));
    currentSquares.attr('class', d => `rack-square tile letter-${d && d.char}`)
    currentSquares.classed('data-letter', d => d && d.char);
    currentSquares.classed("empty", d => d === undefined);
    currentSquares.classed("used", d => d && d.used)

    let component = this;
    currentSquares.on("click", function(e, d) {
      component.owner.unfocus();
      component.cursor = d;
    });

    currentSquares.classed("cursor", (d, i) => this.cursor === i);
    squares.exit().remove();
  }

  get cursor() {
    return this._cursor;
  }

  set cursor(idx) {
    this._cursor = idx;
    this.draw();
  }

  _populateSquares() {
  }

  _lastEmptyIndex() {
    for (let i = (this.size - 1); i > 0; i--) {
      if (this.squares[i] === undefined) { return i }
    }
  }

  _setLastEmptySquare(e, i) {
    let idx = this._lastEmptyIndex();
    this.squares[idx] = { char: e, i: i };
  }
}

export default Rack;
