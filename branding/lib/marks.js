"use strict";

/*
 * Builds every Lymra logo variant as SVG markup from one shared geometry.
 *
 * The mark is a square silhouette cut into an "L" and a data stack of blocks
 * in its corner. Flat colors only: vermilion, with the top block, the most
 * recently pushed value, in gold. Every variant has a transparent background.
 * The animated variant pushes the blocks one by one, with gold always marking
 * the top, then frees them all at once, the way a scope exit frees a whole
 * data stack with a single store.
 */

const wordmark = require("./wordmark-path");

const PALETTE = Object.freeze({
    vermilion: "#E8452C",
    gold: "#F5A524",
    charcoal: "#1C1917",
    white: "#FFFFFF",
});

/*
 * The mark is fully described by these numbers. `origin` and `size` place the
 * square silhouette, `stroke` is the L's thickness, `gap` is the width of every
 * cut, and the radii round the silhouette's outer corners and everything else.
 */
const DETAILED = Object.freeze({
    viewBox: "64 64 384 384",
    origin: 96,
    size: 320,
    stroke: 92,
    gap: 12,
    outerRadius: 36,
    innerRadius: 6,
    blockCount: 3,
});

// Thicker strokes, wider cuts and two blocks, so the mark stays crisp at 16 to 32 px.
const SIMPLIFIED = Object.freeze({
    viewBox: "80 80 352 352",
    origin: 96,
    size: 320,
    stroke: 104,
    gap: 20,
    outerRadius: 40,
    innerRadius: 8,
    blockCount: 2,
});

const ANIMATION_SECONDS = 4;

// Keyframe percentages at which each block starts falling and lands, bottom block first.
const PUSH_WINDOWS = Object.freeze([[10, 18], [28, 36], [46, 54]]);
const HOLD_UNTIL = 80;
const FREED_AT = 84;

// A rectangle path with its own radius per corner, listed clockwise from top left.
function roundedRectPath(x, y, width, height, [topLeft, topRight, bottomRight, bottomLeft]) {
    return `M${x + topLeft} ${y}H${x + width - topRight}`
        + `A${topRight} ${topRight} 0 0 1 ${x + width} ${y + topRight}V${y + height - bottomRight}`
        + `A${bottomRight} ${bottomRight} 0 0 1 ${x + width - bottomRight} ${y + height}H${x + bottomLeft}`
        + `A${bottomLeft} ${bottomLeft} 0 0 1 ${x} ${y + height - bottomLeft}V${y + topLeft}`
        + `A${topLeft} ${topLeft} 0 0 1 ${x + topLeft} ${y}Z`;
}

// The L: outer corners take the silhouette radius, the stem and foot tips the small one.
function letterPath({ origin, size, stroke, outerRadius: big, innerRadius: small }) {
    const far = origin + size;
    const footTop = far - stroke;
    return `M${origin + big} ${origin}H${origin + stroke - small}`
        + `A${small} ${small} 0 0 1 ${origin + stroke} ${origin + small}V${footTop}`
        + `H${far - small}A${small} ${small} 0 0 1 ${far} ${footTop + small}V${far - big}`
        + `A${big} ${big} 0 0 1 ${far - big} ${far}H${origin + big}`
        + `A${big} ${big} 0 0 1 ${origin} ${far - big}V${origin + big}`
        + `A${big} ${big} 0 0 1 ${origin + big} ${origin}Z`;
}

/*
 * Stack blocks from the bottom up, filling the corner between stem and foot.
 * The top block's top right corner completes the square silhouette.
 */
function blockPaths(geometry) {
    const { origin, size, stroke, gap, blockCount, outerRadius, innerRadius } = geometry;
    const x = origin + stroke + gap;
    const width = origin + size - x;
    const stackBottom = origin + size - stroke - gap;
    const height = (stackBottom - origin - gap * (blockCount - 1)) / blockCount;
    if (!(height > 0)) {
        throw new Error("Stroke and gap leave no room for the stack blocks");
    }
    return Array.from({ length: blockCount }, (_, index) => {
        const y = stackBottom - height * (index + 1) - gap * index;
        const isTop = index === blockCount - 1;
        const radii = [innerRadius, isTop ? outerRadius : innerRadius, innerRadius, innerRadius];
        return roundedRectPath(x, y, width, height, radii);
    });
}

function svgDocument(viewBox, body) {
    return [
        `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}" role="img" aria-labelledby="title">`,
        '<title id="title">Lymra</title>',
        body,
        "</svg>",
        "",
    ].join("\n");
}

// The letter plus its stack; `ink` fills everything except the top block, which gets `accent`.
function glyph(geometry, { ink, accent }) {
    const blocks = blockPaths(geometry);
    const shapes = blocks.map((path, index) => {
        const fill = index === blocks.length - 1 ? accent : ink;
        return `<path d="${path}" fill="${fill}"/>`;
    });
    return [`<path d="${letterPath(geometry)}" fill="${ink}"/>`, ...shapes].join("\n");
}

const COLOR = Object.freeze({ ink: PALETTE.vermilion, accent: PALETTE.gold });
const WHITE = Object.freeze({ ink: PALETTE.white, accent: PALETTE.gold });
const MONO = Object.freeze({ ink: "currentColor", accent: "currentColor" });

const icon = () => svgDocument(DETAILED.viewBox, glyph(DETAILED, COLOR));
const smallIcon = () => svgDocument(SIMPLIFIED.viewBox, glyph(SIMPLIFIED, COLOR));
const markWhite = () => svgDocument(DETAILED.viewBox, glyph(DETAILED, WHITE));
// Single ink that follows the surrounding text color when inlined in HTML.
const markMonochrome = () => svgDocument(DETAILED.viewBox, glyph(DETAILED, MONO));

const WORDMARK_GAP = 72;
const WORDMARK_CAP_HEIGHT = 220;
const WORDMARK_EDGE_PAD = 16;

/*
 * Sets the word on the mark's baseline so the text sits level with the L's foot.
 * The canvas grows downward only as far as the descender of the "y" needs.
 */
function wordmarkLayout() {
    const [originX, originY, size] = DETAILED.viewBox.split(" ").map(Number);
    const markEdge = DETAILED.origin + DETAILED.size;
    const scale = WORDMARK_CAP_HEIGHT / wordmark.capHeight;
    const textX = markEdge + WORDMARK_GAP;
    const width = Math.ceil(textX + wordmark.width * scale + WORDMARK_EDGE_PAD - originX);
    const descender = Math.ceil(markEdge + wordmark.descent * scale + WORDMARK_EDGE_PAD);
    const bottom = Math.max(originY + size, descender);
    return {
        viewBox: `${originX} ${originY} ${width} ${bottom - originY}`,
        transform: `translate(${textX} ${markEdge}) scale(${Number(scale.toFixed(6))})`,
    };
}

function wordmarkWith(textColor) {
    const { viewBox, transform } = wordmarkLayout();
    const body = [
        glyph(DETAILED, COLOR),
        `<path d="${wordmark.path}" fill="${textColor}" transform="${transform}"/>`,
    ];
    return svgDocument(viewBox, body.join("\n"));
}

function keyframeRule(name, stops) {
    const lines = stops.map(([percents, declarations]) => `${percents} { ${declarations} }`);
    return `@keyframes ${name} {\n${lines.join("\n")}\n}`;
}

/*
 * A block falls in gold, stays gold while it is the top of the stack, and
 * turns vermilion as the next block lands on it. The last block stays gold.
 */
function pushKeyframes(index) {
    const [start, land] = PUSH_WINDOWS[index];
    const next = PUSH_WINDOWS[index + 1];
    const shown = "opacity: 1; transform: none;";
    const restingFill = next ? PALETTE.vermilion : PALETTE.gold;
    const stops = [[`0%, ${start}%`, `opacity: 0; transform: translateY(-48px); fill: ${PALETTE.gold};`]];
    if (next) {
        stops.push([`${land}%, ${next[0]}%`, `${shown} fill: ${PALETTE.gold};`]);
        stops.push([`${next[1]}%, ${HOLD_UNTIL}%`, `${shown} fill: ${restingFill};`]);
    } else {
        stops.push([`${land}%, ${HOLD_UNTIL}%`, `${shown} fill: ${restingFill};`]);
    }
    stops.push([`${FREED_AT}%, 100%`, `opacity: 0; transform: none; fill: ${restingFill};`]);
    return keyframeRule(`push-${index}`, stops);
}

/*
 * CSS animation rather than SMIL, so the reduced motion media query can stop it.
 * With motion reduced, the static full-stack icon is shown.
 */
function animatedIcon() {
    const geometry = DETAILED;
    if (geometry.blockCount !== PUSH_WINDOWS.length) {
        throw new Error("Each stack block needs exactly one push window");
    }
    const easing = "cubic-bezier(0.2, 0.8, 0.2, 1)";
    const style = [
        "<style>",
        `.push { animation: ${ANIMATION_SECONDS}s ${easing} infinite both; transform-box: fill-box; }`,
        ...PUSH_WINDOWS.map((_, index) => `.push-${index} { animation-name: push-${index}; }`),
        ...PUSH_WINDOWS.map((_, index) => pushKeyframes(index)),
        "@media (prefers-reduced-motion: reduce) { .push { animation: none; } }",
        "</style>",
    ].join("\n");

    const blocks = blockPaths(geometry).map((path, index) => {
        const fill = index === geometry.blockCount - 1 ? PALETTE.gold : PALETTE.vermilion;
        return `<path d="${path}" fill="${fill}" class="push push-${index}"/>`;
    });
    const body = [style, `<path d="${letterPath(geometry)}" fill="${PALETTE.vermilion}"/>`, ...blocks];
    return svgDocument(geometry.viewBox, body.join("\n"));
}

// Every static variant, with the PNG widths to export for each.
const VARIANTS = Object.freeze([
    { name: "lymra-icon", build: icon, rasterSizes: [48, 64, 96, 128, 180, 192, 256, 512, 1024] },
    { name: "lymra-icon-small", build: smallIcon, rasterSizes: [16, 24, 32] },
    { name: "lymra-mark-white", build: markWhite, rasterSizes: [64, 128, 256, 512, 1024] },
    { name: "lymra-mark-mono", build: markMonochrome, rasterSizes: [64, 128, 256, 512, 1024] },
    { name: "lymra-wordmark", build: () => wordmarkWith(PALETTE.charcoal), rasterSizes: [400, 800, 1600] },
    { name: "lymra-wordmark-white", build: () => wordmarkWith(PALETTE.white), rasterSizes: [400, 800, 1600] },
]);

module.exports = Object.freeze({
    VARIANTS,
    ANIMATION_SECONDS,
    animatedIcon,
});
