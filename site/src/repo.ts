export const repo = __REPO_URL__;
export const repoName = repo.split("/").pop() ?? "mophila";
// 何ファイルかに分かれているモジュールは置き場を指すので、その場合は tree
export const blob = (path: string): string => `${repo}/${path.includes(".") ? "blob" : "tree"}/main/${path}`;
