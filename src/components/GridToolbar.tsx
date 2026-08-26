import { useTranslation } from "react-i18next";

interface Props {
  selectedCount: number;
  totalRows: number;
  canDelete: boolean; // 是否有主键
  onAdd: () => void;
  onDelete: () => void;
  onSave: () => void;
  onRefresh: () => void;
}

export default function GridToolbar({
  selectedCount,
  totalRows,
  canDelete,
  onAdd,
  onDelete,
  onSave,
  onRefresh,
}: Props) {
  const { t } = useTranslation();

  return (
    <div className="grid-toolbar">
      <button className="btn-icon" onClick={onAdd} title={t("grid.toolbar.add")}>
        ➕ {t("grid.toolbar.add")}
      </button>
      <button
        className="btn-icon"
        onClick={onDelete}
        disabled={!canDelete || selectedCount === 0}
        title={t("grid.toolbar.delete")}
      >
        ➖ {t("grid.toolbar.delete")}
      </button>
      <button className="btn-icon" onClick={onSave} title={t("grid.toolbar.save")}>
        💾 {t("grid.toolbar.save")}
      </button>
      <button className="btn-icon" onClick={onRefresh} title={t("grid.toolbar.refresh")}>
        🔄 {t("grid.toolbar.refresh")}
      </button>
      <span className="toolbar-info">
        {selectedCount > 0 && `${t("grid.toolbar.selected", { count: selectedCount })} · `}
        {t("grid.toolbar.totalRows", { count: totalRows })}
      </span>
    </div>
  );
}
