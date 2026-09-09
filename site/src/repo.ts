export const repo = __REPO_URL__;
export const repoName = repo.split("/").pop() ?? "mophila";
export const blob = (path: string): string => `${repo}/blob/main/${path}`;
