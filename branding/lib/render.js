"use strict";

const { chromium } = require("playwright");

function aspectRatio(svg) {
    const match = svg.match(/viewBox="([\d.\s-]+)"/);
    if (!match) {
        throw new Error("SVG has no viewBox, so its aspect ratio is unknown");
    }
    const [, , width, height] = match[1].trim().split(/\s+/).map(Number);
    if (!(width > 0 && height > 0)) {
        throw new Error(`Invalid viewBox "${match[1]}"`);
    }
    return width / height;
}

// Inlines the SVG so its CSS animations are reachable through document.getAnimations().
function pageMarkup(svg) {
    const sized = svg.replace("<svg ", '<svg width="100%" height="100%" ');
    return `<!doctype html><html><body style="margin:0;background:transparent;color:#000">${sized}</body></html>`;
}

/*
 * Opens one headless browser and reuses it for every export.
 * Call close() when done, even after a failure.
 */
async function openRenderer() {
    const browser = await chromium.launch();
    const page = await browser.newPage({ deviceScaleFactor: 1 });

    async function load(svg, width) {
        if (!Number.isInteger(width) || width <= 0) {
            throw new Error(`Raster width must be a positive integer, got ${width}`);
        }
        const height = Math.round(width / aspectRatio(svg));
        await page.setViewportSize({ width, height });
        await page.setContent(pageMarkup(svg));
    }

    async function png(svg, width) {
        await load(svg, width);
        return page.screenshot({ omitBackground: true, type: "png" });
    }

    // Yields one PNG per frame with every animation paused at that frame's time.
    async function* frames(svg, width, seconds, fps) {
        await load(svg, width);
        const frameCount = Math.round(seconds * fps);
        for (let frame = 0; frame < frameCount; frame += 1) {
            const milliseconds = (frame * 1000) / fps;
            await page.evaluate((time) => {
                for (const animation of document.getAnimations()) {
                    animation.pause();
                    animation.currentTime = time;
                }
            }, milliseconds);
            yield await page.screenshot({ omitBackground: true, type: "png" });
        }
    }

    return {
        png,
        frames,
        close: () => browser.close(),
    };
}

module.exports = Object.freeze({ openRenderer });
