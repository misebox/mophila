import { Show, type Component } from "solid-js";
import { Faded, SideIndex, WithSide } from "@/parts";
import { route } from "@/route";
import { BuiltinContent, builtinGroups, builtinLabel } from "@/pages/Builtins";
import { LibraryContent, libraryGroups, libraryLabel } from "@/pages/Library";

// 組み込みとライブラリは同じページ。目次は続けて並べ、URL は #/builtins/… と #/lib/… のまま
const isLib = (): boolean => route().page === "lib";

export const Reference: Component = () => (
  <WithSide
    current={isLib() ? libraryLabel() : builtinLabel()}
    side={<SideIndex groups={[...builtinGroups(), ...libraryGroups()]} />}
  >
    <Faded key={`${route().page}/${route().sub}`}>
      {() => (
        <Show when={isLib()} fallback={<BuiltinContent id={route().sub || "functions"} />}>
          <LibraryContent id={libraryLabel()} />
        </Show>
      )}
    </Faded>
  </WithSide>
);
