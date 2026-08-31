export interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
}

export type ThemeName = "light" | "dark" | "book";
export type ViewMode = "preview" | "split" | "editor";
