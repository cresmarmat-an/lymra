"use strict";

const HEADER_BYTES = 6;
const ENTRY_BYTES = 16;
const ICON_TYPE = 1;
const MAX_ICO_SIZE = 256;

/*
 * Packs PNG images into a Windows .ico container. Modern ICO readers accept
 * PNG payloads directly, so no bitmap conversion is needed.
 * Each image is { size, data } where size is the square edge in pixels.
 */
function buildIco(images) {
    if (images.length === 0) {
        throw new Error("An .ico file needs at least one image");
    }
    const header = Buffer.alloc(HEADER_BYTES);
    header.writeUInt16LE(0, 0);
    header.writeUInt16LE(ICON_TYPE, 2);
    header.writeUInt16LE(images.length, 4);

    let offset = HEADER_BYTES + ENTRY_BYTES * images.length;
    const entries = images.map(({ size, data }) => {
        if (!Number.isInteger(size) || size < 1 || size > MAX_ICO_SIZE) {
            throw new Error(`ICO image size must be 1 to ${MAX_ICO_SIZE}, got ${size}`);
        }
        const entry = Buffer.alloc(ENTRY_BYTES);
        // A stored 0 means 256 pixels.
        entry.writeUInt8(size % MAX_ICO_SIZE, 0);
        entry.writeUInt8(size % MAX_ICO_SIZE, 1);
        entry.writeUInt16LE(1, 4);
        entry.writeUInt16LE(32, 6);
        entry.writeUInt32LE(data.length, 8);
        entry.writeUInt32LE(offset, 12);
        offset += data.length;
        return entry;
    });

    return Buffer.concat([header, ...entries, ...images.map((image) => image.data)]);
}

module.exports = Object.freeze({ buildIco });
