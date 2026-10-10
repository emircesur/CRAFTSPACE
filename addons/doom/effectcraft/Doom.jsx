// Doom.jsx: plays Doom in EffectCraft, a move at a time (Window › Doom.jsx).
//
// Click the buttons, or click in the box and type: W/S forward and back, A/D turn, Q/E strafe,
// F fire, Space use (doors, switches), R forward and fire, X turn around, 1-5 weapons,
// . wait. Each move is five tics (a seventh of a second); Undo takes it back.
//
// The moves go into the Doom effect's move log ("Moves" and "Move Log 1…48"), on a layer
// named Doom in the active comp (made if there isn't one). Set the effect's Show to
// "Replay over Time" to play the run back on the timeline, and render it like any comp.
//
// Part of the CraftSpace Doom add-on. SPDX-License-Identifier: GPL-2.0-or-later
(function (thisObj) {
  var EFFECT = "org.craftspace.doom";
  var PER_SLIDER = 13;
  var SLIDERS = 48;
  var MAX_MOVES = PER_SLIDER * SLIDERS;
  var KEYS = { w: 1, s: 2, a: 3, d: 4, q: 5, e: 6, f: 7, " ": 8, r: 9, "1": 10, "2": 11, "3": 12, "4": 13, "5": 14, x: 15, ".": 0 };

  function doomEffect(make) {
    var comp = app.project.activeItem;
    if (!(comp instanceof CompItem)) {
      if (!make) return null;
      comp = app.project.items.addComp("Doom", 640, 400, 1, 120, 35);
    }
    for (var i = 1; i <= comp.numLayers; i++) {
      var effects = comp.layer(i).property("ADBE Effect Parade");
      if (!effects) continue;
      for (var j = 1; j <= effects.numProperties; j++) {
        var fx = effects.property(j);
        if (fx.matchName === EFFECT || fx.name === "Doom") return fx;
      }
    }
    if (!make) return null;
    var solid = comp.layers.addSolid([0, 0, 0], "Doom", comp.width, comp.height, 1);
    return solid.property("ADBE Effect Parade").addProperty(EFFECT);
  }

  function readMoves(fx) {
    var n = Math.round(fx.property("Moves").value);
    var moves = [];
    for (var s = 0; s * PER_SLIDER < n; s++) {
      var v = fx.property("Move Log " + (s + 1)).value;
      for (var k = 0; k < PER_SLIDER && moves.length < n; k++) {
        moves.push(v % 16);
        v = Math.floor(v / 16);
      }
    }
    return moves;
  }

  // Writes the slider holding move `index` (and the count).
  function writeSlider(fx, moves, index) {
    var s = Math.floor(index / PER_SLIDER);
    var v = 0;
    for (var k = Math.min(moves.length, (s + 1) * PER_SLIDER) - 1; k >= s * PER_SLIDER; k--) v = v * 16 + moves[k];
    fx.property("Move Log " + (s + 1)).setValue(v);
    fx.property("Moves").setValue(moves.length);
  }

  function play(code, status) {
    app.beginUndoGroup("Doom Move");
    try {
      var fx = doomEffect(true);
      var moves = readMoves(fx);
      if (moves.length >= MAX_MOVES) {
        status.text = "This run is full (" + MAX_MOVES + " moves). Start a new one on another layer.";
        return;
      }
      moves.push(code);
      writeSlider(fx, moves, moves.length - 1);
      status.text = "Move " + moves.length + " of " + MAX_MOVES;
    } catch (err) {
      status.text = String(err);
    } finally {
      app.endUndoGroup();
    }
  }

  function newGame(status) {
    var fx = doomEffect(false);
    if (!fx) return;
    app.beginUndoGroup("Doom New Game");
    fx.property("Moves").setValue(0);
    for (var s = 1; s <= SLIDERS; s++) fx.property("Move Log " + s).setValue(0);
    app.endUndoGroup();
    status.text = "New game";
  }

  var ui = thisObj instanceof Panel ? thisObj : new Window("palette", "Doom", undefined, { resizeable: true });
  ui.orientation = "column";
  ui.alignChildren = ["fill", "top"];
  var keys = ui.add("edittext", undefined, "", { name: "keys" });
  keys.helpTip = "Click here and type: W A S D, Q E strafe, F fire, Space use, R forward+fire, X turn around, 1-5 weapons";
  var status = ui.add("statictext", undefined, "Click the box and type W A S D (or use the buttons).", { name: "status" });
  var rows = [
    [["Strafe left (Q)", 5], ["Forward (W)", 1], ["Strafe right (E)", 6]],
    [["Turn left (A)", 3], ["Back (S)", 2], ["Turn right (D)", 4]],
    [["Fire (F)", 7], ["Use (Space)", 8], ["Fwd + fire (R)", 9]],
    [["Turn around (X)", 15], ["Wait (.)", 0]]
  ];
  for (var r = 0; r < rows.length; r++) {
    var row = ui.add("group");
    row.orientation = "row";
    row.alignChildren = ["fill", "center"];
    for (var c = 0; c < rows[r].length; c++) {
      (function (label, code) {
        var b = row.add("button", undefined, label, { name: "move" + code });
        b.onClick = function () { play(code, status); };
      })(rows[r][c][0], rows[r][c][1]);
    }
  }
  var weapons = ui.add("group");
  weapons.orientation = "row";
  for (var w = 1; w <= 5; w++) {
    (function (n) {
      var b = weapons.add("button", undefined, String(n), { name: "weapon" + n });
      b.helpTip = ["Fist / chainsaw", "Pistol", "Shotgun", "Chaingun", "Rocket launcher"][n - 1];
      b.onClick = function () { play(9 + n, status); };
    })(w);
  }
  var restart = ui.add("button", undefined, "New Game", { name: "newGame" });
  restart.onClick = function () { newGame(status); };

  // Typing: each new character is a move; the box is emptied as it goes.
  keys.onChanging = function () {
    var text = keys.text;
    if (!text) return;
    keys.text = "";
    for (var i = 0; i < text.length; i++) {
      var code = KEYS[text.charAt(i).toLowerCase()];
      if (code !== undefined) play(code, status);
    }
  };

  if (ui instanceof Window) {
    ui.center();
    ui.show();
  } else {
    ui.layout.layout(true);
  }
})(this);
