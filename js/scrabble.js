// Import the D3 packages we want to use
import { select } from 'd3-selection';
import Rack from './rack';

const boni = {
  "quad_word": "4x word",
  "double_word": "2x word",
  "double_letter": "2x letter",
  "triple_word": "3x word",
  "triple_letter": "3x letter",
  "quad_letter": "4x letter",
};

class Scrabble {
  constructor(socket) {
    window.game = this
    // FIXME: remove token logic (now handled in user_socket.ex instead of the game channel)
    this.token = select("meta[name='id-token']").attr("content");
    this.player = select("meta[name='player']").attr("content");
    this.game_id = select("meta[name='game-id']").attr("content");

    this.socket = socket
    this.cursor = -1;
    this.size = 15;
    this.element = select("section#board-container");
    this.header = select("section#board-header");
    this.rack_container = select("section.rack-container");
    this.flash_container = select("section.flash-container p");
    this.footer = select("section#board-footer");
    this.direction = 'h'; // or 'v';
    this._current_player = undefined;
    this.proposed = {};
    this.scores = {};
    this._players = [];
    this.playersOnline = [];
    this.first_load = true;

    this._rack = new Rack(this, "#rack-container", []);

    this.joinGameAs(this.token, this.name);

    Notification.requestPermission();
  }

  get players() {
    return this._players;
  }

  set players(newPlayers) {
    this._players = newPlayers;

    if (newPlayers.indexOf(this.player) >= 0) { // this should always be tru
      while (this._players.indexOf(this.player) > 0) {
        // always list the user's player first
        this._players.unshift(this._players.pop())
      }
    }
  }

  joinGameAs(token, name) {
    this.channel = this.socket.channel(`game:${this.game_id}`, { token: token });
    window.channel = this.channel;
    this.channel.join()
      .receive("ok", resp => { console.log(`joined game:${this.game_id}`, resp) })
      .receive("error", resp => { console.error("unable to join", resp) })

    this.channel.on("player-state", ({game, rack, remaining}) => {
      if (game) { this.handleGameState({ game }) }
      if (rack) { this.handleRack({ rack }) }
      if (remaining) { this.handleRemaining({ remaining }) }
    });

    this.channel.on("new_proposed", payload => {
      if (this.current_player === this.player) { return }
      this.handleProposed(payload);
    });

    this.channel.on("error", payload => {
      this.flash("error", payload);
    });

    this.channel.on("info", payload => {
      this.flash("info", payload);
    });

    this.channel.on("log", payload => {
      console.log(payload)
    });

    this.channel.on("presence", payload => {
      this.playersOnline = payload.online;
      this.drawScores();
    });

    this.channel.on("rack", payload => {
      this.handleRack(payload);
    });

    this.didReceiveAttrs();
  }

  flash(level, { message }) {
    message = message || "&nbsp;"
    this.flash_container.html(message);
    this.flash_container.classed("alert-danger", level === "error");
  }

  handleProposed(payload) {
    this.proposed = payload;
    this.drawSquares();
  }

  get current_player() {
    return this._current_player;
  }

  set current_player(newPlayer) {
    this._current_player = newPlayer;

    if (this.first_load) { return }
    if (document.hasFocus()) { return }

    if (this.player === this._current_player) {
      if (this.notification !== undefined) { this.notification.close() }

      if (Notification.permission === "granted") {
        this.notification = new Notification("ScrabbleEx", { body: "It is your turn" });
        this.notification.onclick = function() {
          window.focus();
        }
      }
    }
  }

  handleGameState(payload) {
    let game;
    if (payload && payload.game) {
      game = payload.game
    } else {
      game = payload
    }

    if (game.current_player) {
      this.current_player = game.current_player // FIXME: where does this logic belong?
    }


    this.element.classed(game.board_type, true);
    this.flash_container.classed(game.board_type, true);

    if (game.size) { this.size = game.size; }

    if (game.board) {
      this.proposed = {};
      this.data = game.board;
      this.last_turn_indices = game.last_turn_indices || [];
    }

    this.scores = game.scores;
    this.players = game.players;

    this.gameOver = game.game_over;
    this.passAllowed = game.pass_allowed;
    this.swapAllowed = game.swap_allowed;

    if (this.gameOver) {
      this.current_player = null;
      this.flash("info", { message: "game over!" });
    }

    this.drawSquares();
    this.drawScores();
    this.drawStartButton();
    this.drawSubmitButton();
    this.drawSwapButton();
    this.drawPassButton();
    this.first_load = false;
  }

  handleRack({ rack, remaining }) {
    if (rack) {
      this.rack = rack;
    }

    if (remaining) { // FIXME: remove
      this.handleRemaining(remaining)
    }
  }

  handleRemaining({ remaining }) {
    if (remaining) {
      let formatted = remaining.map(f => f.join("&ndash;")).join("<br />");
      select('#remaining-letters').html(formatted);
    }
  }

  get rack() {
    return this._rack;
  }

  set rack(newRack) {
    this._rack.letters = newRack;
  }

  didReceiveAttrs() {
    let component = this;
    select(window).on('keydown', function() {
      if (component.rack.hasFocus()) {
        return component.rack.handleKeydown(event);
      }

      if (component.player !== component.current_player) {
        return;
      }

      if (event.ctrlKey) {
        return;
      }

      switch (event.key) {
        case "ArrowUp": {
          event.preventDefault();
          return component.moveByArrow(0, -1);
        }
        case "ArrowDown": {
          event.preventDefault();
          return component.moveByArrow(0, 1);
        }
        case "ArrowLeft": {
          event.preventDefault();
          return component.moveByArrow(-1, 0);
        }
        case "ArrowRight": {
          event.preventDefault();
          return component.moveByArrow(1, 0);
        }
        case "Enter": {
          event.preventDefault();
          return component.submitProposed();
        }
      }

      if (event.which === 8) {
        component.setProposed(null);
      }

      if (event.which >= 65 && event.which <= 90) {
        component.setProposed(String.fromCharCode(event.which));
      }
    });
  }

  unfocus() {
    this.setCursor(undefined);
    this.drawSquares(); // FIXME: only if cursor changes
  }

  moveByArrow(dx, dy) {
    let want;

    if (dy !== 0) {
      want = 'v'
    } else {
      want = 'h'
    }

    if (want === this.direction) {
      return this.moveCursor(dx, dy);
    } else {
      this.direction = want;
      if (this.currentTileSelection().html() === "") { // FIXME: used proposed data
        return this.setCursor(this.cursor); // stay here, but change direction
      } else {
        return this.moveCursor(dx, dy);
      }
    }
  }

  currentTileSelection() {
    return select(`#tile-${this.cursor}`);
  }

  moveCursor(dx, dy) {
    let x = this.cursorX() + dx;
    let y = this.cursorY() + dy;

    if ((dx > 0 && x > (this.size - 1)) || (dx < 0 && x < 0)) {
      return false;
    }

    if ((dy > 0 && y > (this.size - 1)) || (dy < 0 && y < 0)) {
      return false;
    }

    this.setCursorXY(x, y);
    return true;
  }

  setCursorXY(x, y) {
    this.setCursor((this.size * y) + x);
  }

  cursorX() {
    return this.cursor % this.size;
  }

  cursorY() {
    return Math.floor(this.cursor / this.size);
  }

  setProposed(char) {
    if (this.data[this.cursor].character !== undefined) {
      return this.reverseCursor();
    }

    let tile = select(`#tile-${this.cursor}`)

    if (char === null) {
      if (tile.html() === "") { // back up to last set tile
        if (this.reverseCursor()) {
          return this.setProposed(null);
        }
      }

      let component = this;

      tile.html(function ({ bonus }) {
        return boni[bonus];
      }).classed("tile-proposed", false)
        .classed("tile-blank", false);
      this.deleteProposed(this.cursor);
      this.reverseCursor();
    } else {
      if (this.updateProposed(this.cursor, char)) {
        let isBlank = this.proposed[this.cursor][0] === ":"
        tile.html(this.proposed[this.cursor]).classed("tile-proposed", true)
          .classed("tile-blank", isBlank);
        this.advanceCursor();
      }
    }
  }

  deleteProposed(cursor) {
    let d = this.proposed[cursor];
    delete this.proposed[cursor];
    if (d) {
      this.rack.replaceUsedLetter(d);
    }
    this.sendProposed();
  }

  updateProposed(cursor, char) {
    let current = this.proposed[cursor]
    let usingBlank = false;
    let i = this.rack.searchUnusedLetter(char);

    if (!i) {
      return
    }

    usingBlank = i === "BLANK";

    if (current) {
      this.rack.replaceUsedLetter(current);
    }

    this.proposed[cursor] = `${usingBlank ? ":" : ""}${char}`
    this.sendProposed();

    return true;
  }

  push(key, payload) {
    return this.channel.push(key, payload)
      .receive("error", ({ message }) => this.flash("error", { message }));
  }

  submitProposed() {
    this.push("play", this.proposed)
      .receive("ok", (payload) => this.handleRack(payload));
  }

  sendProposed() {
    this.push("proposed", this.proposed)
      .receive("ok", ({ message }) => this.flash("info", { message }))
  }

  sendSwapped() {
    this.push("swap", this.proposed)
      .receive("ok", (payload) => this.handleRack(payload))
  }

  sendPassed() {
    this.push("pass", {});
  }

  clickSetCursor(i) {
    if (this.cursor === i) {
      if (this.direction === "h") {
        this.direction = "v"
      } else {
        this.direction = "h"
      }
    }

    this.setCursor(i);
    this.rack.unfocus();
  }

  setCursor(i) {
    let tile = select(`#tile-${this.cursor}`);
    tile.classed('cursor', false);
    this.cursor = i;

    if (i !== undefined) {
      tile = select(`#tile-${this.cursor}`);
      tile.classed('cursor', true);
      tile.classed('cursor-h', this.direction === "h");
    }
  }

  advanceCursor() {
    if (this.direction === "h") {
      return this.moveCursorToNextEmpty(1, 0);
    } else {
      return this.moveCursorToNextEmpty(0, 1);
    }
  }

  moveCursorToNextEmpty(dx, dy) {
    if (this.moveCursor(dx, dy)) {
      if (this.data[this.cursor].character !== undefined) {
        return this.moveCursorToNextEmpty(dx, dy)
      }

      return true;
    }

    return false;
  }

  reverseCursor() {
    if (this.direction === "h") {
      return this.moveCursor(-1, 0);
    } else {
      return this.moveCursor(0, -1);
    }
  }

  drawSquares() {
    let container = this.element;
    let data = this.data;

    container.classed('current', this.current_player == this.player);

    let squares = container.selectAll('div.board-square').data(data);
    let enterJoin = squares
      .enter()
      .append('div')
      .attr('id', (d, i) => `tile-${i}`);

    enterJoin.on('click', (d, i) => this.clickSetCursor(i))

    let currentSquares = squares.merge(enterJoin);
    currentSquares.attr('class', d => `board-square ${d.bonus || ''} letter-${d && d.character}`)
    currentSquares.classed('bonus', d => !!d.bonus)
    currentSquares.classed("tile", d => d.character);
    currentSquares.classed("tile-blank", d => d.character && d.character[0] == ":");
    currentSquares.classed("tile-proposed", (d, i) => this.proposed[i]);
    currentSquares.classed("last-turn", (_d, i) => this.last_turn_indices.indexOf(i) >= 0);
    currentSquares.filter((d) => d.has_cursor).classed("cursor", true);
    currentSquares.html((d, i) => {
      if (d.character && d.character[0] == ":") {
        return d.character[1];
      } else if (d.bonus) {
        return boni[d.bonus]
      } else {
        return d.character || this.proposed[i] || "";
      }
    });
    squares.exit().remove();
  }

  drawSubmitButton() {
    let data = [];
    if (this.player === this.current_player) {
      data.push(0);
    }

    let button = select('#submit-button-container').selectAll('button.submit-button').data(data);
    button.enter()
      .append('button')
      .attr('class', 'submit-button')
      .html('SUBMIT')
      .on("click", () => { this.submitProposed() });

    button.exit().remove();
  }

  playerIsOffline(player) {
    return !this.playerIsOnline(player);
  }

  playerIsOnline(player) {
    return this.playersOnline.indexOf(player) >= 0;
  }

  drawScores() {
    // FIXME: this is a mess
    let scoreCount = 0;
    for (let player in this.scores) {
      scoreCount = Math.max(this.scores[player].length, scoreCount);
    }

    // count of all rows needed
    let data = [];
    for (let i = 0; i < scoreCount; i++) {
      data.push(i);
    }

    let component = this;
    let scores = this.scores;
    let players = this.players;

    let table = select('#score-container table');
    let headerSelection = table.select('thead').selectAll('tr').data([0]);
    let headEnter = headerSelection.enter()
      .append('tr').attr('id', 'score-header')

    headerSelection = headerSelection.merge(headEnter);
    let thSelection = headerSelection.selectAll('th').data(players);
    thSelection = thSelection.merge(thSelection.enter().append('th'));
    thSelection.html(function (player) {
      return `${player} (${component.totalScore(player)})`
    }).classed('current-player', (d) => this.current_player == d)
    .classed('is-player', (d) => this.player == d)
    .classed('offline', (d) => this.playerIsOffline(d));

    let rowSelection = table.select('tbody').selectAll('tr').data(data);
    let row_entry = rowSelection.enter().append('tr')
    let rows = rowSelection.merge(row_entry);
    rows.each(function(d) {
      let cells = select(this).selectAll('td').data(players)
      let cell_entry = cells.enter().append('td');
      cells = cells.merge(cell_entry);

      cells.html(function (p) {
        let localScores = scores[p][d] || [];

        let total = localScores.reduce((acc, score) => {
          return acc + score[1]
        }, 0);

        let scoreString = localScores.map(([word, points]) => {
          if (word === "*") {
            return `<span class='score-bingo'>BINGO: ${points}</span>`
          }

          if (localScores.length > 1) {
            return `<span class='score-word'>${word}: ${points}</span>`
          } else {
            return `<span class='score-word'>${word}</span>`
          }
        }).join("");

        return `<b>${total}</b>:${scoreString}`
      });
    })
  }

  totalScore(player) {
    let sum = 0;
    this.scores[player].forEach(turn => {
      turn.forEach(word => {
        sum += word[1]
      })
    })

    return sum;
  }

  drawStartButton() {
    if (this.gameOver) {
      return;
    }

    let data = [];
    if (!this.current_player) {
      data.push(0);
    }

    let selection = this.header.selectAll('div').data(data);
    let channel = this.channel

    let el = selection.enter()
      .append('div')

    el.append('button')
      .attr('class', 'broadcast-button')
      .html('invite')
      .on('click', () => { channel.push("broadcast_invite") });

    el.append('br')

    el.append('button')
      .attr('class', 'start-button')
      .html("click here to start after everyone has joined")
      .on("click", () => { channel.push("start") });

    selection.exit().remove();
  }

  drawPassButton() {
    let data = [];
    if (this.passAllowed && this.current_player === this.player) {
      data.push(0);
    }
    let selection = select('#submit-button-container').selectAll('button#pass-button').data(data);
    let component = this;
    selection.enter()
      .append('button')
      .attr('id', 'pass-button')
      .html("PASS")
      .on('click', () => {
        if (confirm("Your turn will be over. Proceed?")) {
          component.sendPassed();
        }
      });

    selection.exit().remove();
  }

  drawSwapButton() {
    let data = [];
    if (this.swapAllowed && this.current_player === this.player) {
      data.push(0);
    }

    let selection = select('#submit-button-container').selectAll('button#swap-button').data(data);

    let component = this;
    selection.enter()
      .append('button')
      .attr('id', 'swap-button')
      .html("SWAP")
      .on('click', () => {
        if (Object.keys(component.proposed).length === 0) {
          alert("Type the letters you want to swap somewhere into the board, then click the swap button again.");
        } else {
          if (confirm("The letters you have typed into the board will be swapped. Proceed?")) {
            component.sendSwapped();
          }
        }
      });

    selection.exit().remove();
  }
}

export default Scrabble;
