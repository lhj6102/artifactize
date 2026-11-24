// The readable text Point appends for a place in a document (J-POINT): its line and column,
// `L42:C9`, led by the file's path when the Artifact holds several files.
export function position(path: string | null, line: number, column: number): string {
  const place = `L${line}:C${Math.max(1, column)}`;
  return path ? `${path} ${place}` : place;
}
