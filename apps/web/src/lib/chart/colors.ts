const PALETTE = [
  "#f8d66d",
  "#d8f96a",
  "#a7f06b",
  "#6ee7a8",
  "#5eead4",
  "#67e8f9",
  "#93c5fd",
  "#c4b5fd",
  "#f0abfc",
];

export function nodeColor(id: string, depth: number, index = 0): string {
  const seed = hash(id) + depth * 17 + index * 7;
  return PALETTE[Math.abs(seed) % PALETTE.length] ?? PALETTE[0];
}

export function nodeMutedColor(depth: number): string {
  return depth % 2 === 0 ? "#3a3f49" : "#484d57";
}

function hash(input: string): number {
  let value = 0;
  for (let index = 0; index < input.length; index += 1) {
    value = (value << 5) - value + input.charCodeAt(index);
    value |= 0;
  }
  return value;
}
