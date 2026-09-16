// Reading a drawn window back as text.
//
// A frame is wrapped to a fixed width, so a sentence a person reads as one sentence is several
// lines with a border down each side. A test that searched the raw frame for a sentence would be
// asserting the wrap point rather than the wording, and would go red the day a label got a word
// longer. These two helpers take the border and the wrapping off, so an assertion is about what
// somebody reads.

/** Every line of a frame, without the blank the frame ends on. */
export const frameLines = (frame: string): string[] => frame.split('\n').filter((line) => line.length > 0);

/** The words inside a frame, as one string, with the border and the wrapping taken off. */
export const frameText = (frame: string): string =>
  frameLines(frame)
    .filter((line) => line.startsWith('│'))
    .map((line) => line.slice(1, -1))
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim();
