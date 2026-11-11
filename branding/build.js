"use strict";

/*
 * Regenerates every Lymra logo asset from lib/marks.js.
 * Requires Node 18+, the playwright package with Chromium, and python3 with Pillow.
 * Usage: node branding/build.js
 */

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const { VARIANTS, ANIMATION_SECONDS, animatedIcon } = require("./lib/marks");
const { openRenderer } = require("./lib/render");
const { buildIco } = require("./lib/ico");
const { encodeAnimation } = require("./lib/encode");

const OUTPUT = Object.freeze({
    svg: path.join(__dirname, "svg"),
    png: path.join(__dirname, "png"),
    animated: path.join(__dirname, "animated"),
    favicon: path.join(__dirname, "favicon.ico"),
});

const FAVICON_SIZES = Object.freeze([16, 32, 48]);
const ANIMATION_FPS = 25;
const ANIMATION_SIZES = Object.freeze([256, 512]);

function resetDirectory(directory) {
    fs.rmSync(directory, { recursive: true, force: true });
    fs.mkdirSync(directory, { recursive: true });
}

async function exportStaticVariants(renderer) {
    for (const variant of VARIANTS) {
        const svg = variant.build();
        fs.writeFileSync(path.join(OUTPUT.svg, `${variant.name}.svg`), svg);
        const variantDirectory = path.join(OUTPUT.png, variant.name);
        fs.mkdirSync(variantDirectory);
        for (const width of variant.rasterSizes) {
            const data = await renderer.png(svg, width);
            fs.writeFileSync(path.join(variantDirectory, `${variant.name}-${width}.png`), data);
        }
    }
}

// Small favicon sizes use the simplified mark; 48 px and up use the detailed one.
async function exportFavicon(renderer) {
    const variants = new Map(VARIANTS.map((variant) => [variant.name, variant]));
    const images = [];
    for (const size of FAVICON_SIZES) {
        const variant = size < 48 ? variants.get("lymra-icon-small") : variants.get("lymra-icon");
        images.push({ size, data: await renderer.png(variant.build(), size) });
    }
    fs.writeFileSync(OUTPUT.favicon, buildIco(images));
}

async function exportAnimation(renderer) {
    const svg = animatedIcon();
    fs.writeFileSync(path.join(OUTPUT.animated, "lymra-icon-animated.svg"), svg);
    for (const size of ANIMATION_SIZES) {
        const frameDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "lymra-frames-"));
        try {
            let index = 1;
            for await (const frame of renderer.frames(svg, size, ANIMATION_SECONDS, ANIMATION_FPS)) {
                fs.writeFileSync(path.join(frameDirectory, `frame-${String(index).padStart(4, "0")}.png`), frame);
                index += 1;
            }
            const outputBase = path.join(OUTPUT.animated, `lymra-icon-animated-${size}`);
            encodeAnimation(frameDirectory, ANIMATION_FPS, outputBase);
        } finally {
            fs.rmSync(frameDirectory, { recursive: true, force: true });
        }
    }
}

async function main() {
    resetDirectory(OUTPUT.svg);
    resetDirectory(OUTPUT.png);
    resetDirectory(OUTPUT.animated);
    const renderer = await openRenderer();
    try {
        await exportStaticVariants(renderer);
        await exportFavicon(renderer);
        await exportAnimation(renderer);
    } finally {
        await renderer.close();
    }
}

main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
});
