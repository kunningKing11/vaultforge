import { UNKNOWN_ASSET_ICON } from "../assetIcons";

/** Shared decorative icon for dropdown options and their selected values. */
export function SelectionIcon({ src }: { src: string }) {
  return (
    <img
      alt=""
      className="selection-icon"
      src={src}
      onError={(event) => {
        const image = event.currentTarget;
        if (image.getAttribute("src") !== UNKNOWN_ASSET_ICON) image.src = UNKNOWN_ASSET_ICON;
      }}
    />
  );
}
