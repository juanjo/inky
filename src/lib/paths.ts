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

export interface Crumb {
  label: string;
  /** Absolute folder this crumb stands for. */
  dir: string;
  /** Inside the library or opened folder, so its contents can be listed. */
  browsable: boolean;
}

/**
 * Folder trail from the visible root (opened folder, else Library) down to the
 * document's own folder. Documents outside both get one crumb: their folder.
 */
export function breadcrumbs(
  path: string,
  roots: { library: string; workspace: string | null; home: string },
): Crumb[] {
  const dir = path.slice(0, path.lastIndexOf("/")) || "/";
  const root =
    roots.workspace && isInside(path, roots.workspace)
      ? { dir: roots.workspace, label: roots.workspace.split("/").pop() || roots.workspace }
      : isInside(path, roots.library)
        ? { dir: roots.library, label: "Library" }
        : null;
  if (!root) return [{ label: displayDir(path, roots.home), dir, browsable: false }];
  const crumbs: Crumb[] = [{ label: root.label, dir: root.dir, browsable: true }];
  let at = root.dir;
  for (const part of dir.slice(root.dir.length).split("/").filter(Boolean)) {
    at = `${at}/${part}`;
    crumbs.push({ label: part, dir: at, browsable: true });
  }
  return crumbs;
}
