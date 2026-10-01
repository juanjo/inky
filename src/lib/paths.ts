/** `path` is `root` or lies beneath it. */
export function isInside(path: string, root: string): boolean {
  if (!root) return false;
  return path === root || path.startsWith(root + "/");
}

/** Parent folder of `path`, with the home folder shortened to `~`. */
export function displayDir(path: string, home: string): string {
  const dir = path.slice(0, path.lastIndexOf("/")) || "/";
  if (home && isInside(dir, home)) return "~" + dir.slice(home.length);
  return dir;
}
