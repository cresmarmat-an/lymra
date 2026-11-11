"use strict";

const { execFileSync } = require("node:child_process");
const path = require("node:path");

const ENCODER_SCRIPT = path.join(__dirname, "encode_animation.py");

// Writes `${outputBase}.gif` and `${outputBase}.webp` from frame-NNNN.png files.
function encodeAnimation(frameDirectory, fps, outputBase) {
    try {
        execFileSync("python3", ["-I", ENCODER_SCRIPT, frameDirectory, String(fps), outputBase], {
            stdio: "inherit",
        });
    } catch (error) {
        if (error.code === "ENOENT") {
            throw new Error("python3 with Pillow is required to encode the animated icons");
        }
        throw error;
    }
}

module.exports = Object.freeze({ encodeAnimation });
