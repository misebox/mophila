import { createSignal } from "solid-js";

export interface Route { page: string; sub: string }

// URL は #/page/sub。sub はページごとの意味 (サンプル名、文書の鍵と見出し、見出しの id)
const parse = (): Route => {
  const [page = "", ...rest] = decodeURIComponent(location.hash).replace(/^#\/?/, "").split("/");
  return { page, sub: rest.join("/") };
};

const [route, setRoute] = createSignal(parse());
window.addEventListener("hashchange", () => setRoute(parse()));

export { route };
export const href = (page: string, sub = ""): string => `#/${page}${sub ? `/${sub}` : ""}`;
