import React, { useEffect, useMemo, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowDownToLine,
  ArrowDown,
  ArrowUp,
  BadgeAlert,
  ChevronDown,
  CheckCircle2,
  FileVideo,
  FolderPlus,
  Library,
  Loader2,
  PlayCircle,
  RefreshCw,
  Search,
  Settings,
  Settings2,
  Trash2,
  UserRound,
} from "lucide-react";
import "./styles.css";

type LibraryFile = {
  id: number;
  name: string;
  path: string;
  parent: string;
  extension: string;
  sizeBytes: number;
  sizeGb: number;
  modified: string;
  modifiedUnix: number;
  depth: number;
  mediaCode: string | null;
  coverPath?: string | null;
  isRootFile: boolean;
};

type ActorVideo = {
  path: string;
  name: string;
  sizeGb: number;
  modified: string;
  modifiedUnix: number;
  mediaCode: string | null;
  coverPath?: string | null;
};

type ActorSummary = {
  name: string;
  path: string;
  fileCount: number;
  totalSizeGb: number;
  modified: string;
  modifiedUnix: number;
  roles: string[];
  videos: ActorVideo[];
};

type DownloadMovePlan = {
  id?: string;
  sourcePath?: string;
  sourceName?: string;
  itemType?: "file" | "folder" | string;
  folder: string;
  folderPath: string;
  targetFileName?: string;
  targetPath?: string;
  targetRelative?: string;
  fileCount: number;
  hasXltd: boolean;
  status: "ready" | "skipped" | "conflict";
  reason: string;
  files: LibraryFile[];
};

type ProcessingOptions = {
  moveToAppreciation: boolean;
  renameEnabled: boolean;
  uppercase: boolean;
  normalizeDash: boolean;
  normalizeUncensoredSuffix: boolean;
  removeAtPrefix: boolean;
  skipIfExists: boolean;
  deleteSourceFolderAfterMove: boolean;
};

type LibrarySettings = {
  rootPath: string;
  downloadPath: string;
  appreciationPath: string;
  coverCapturePercent: number;
  onlineMetadataEnabled: boolean;
  metadataSiteUrl: string;
  metadataBrowser: "auto" | "edge" | "chrome";
};

type DownloadProcessingRequest = {
  id: string;
  sourcePath: string;
  targetPath: string;
  status: string;
};

type OperationLog = {
  id: number;
  timestamp: string;
  action: string;
  sourcePath: string;
  targetPath: string;
  status: "success" | "skipped" | "failed";
  message: string;
};

type OperationResult = {
  success: number;
  skipped: number;
  failed: number;
  logs: OperationLog[];
};

type ArchiveLookup = {
  code: string;
  status: "unsearched" | "searched" | string;
  actorName: string | null;
  actorPath: string | null;
  avatarPath: string | null;
  sourceUrl: string | null;
  message: string;
  searchedAt: string;
};

type ArchiveMovePreview = {
  sourcePath: string;
  targetPath: string;
  actorName: string;
  status: "ready" | "conflict" | "skipped" | string;
  message: string;
};

type ActorProfile = {
  actorName: string;
  actorPath: string;
  avatarPath: string | null;
  avatarSource: string | null;
  updatedAt: string;
};

type MetadataSessionStatus = {
  siteHost: string;
  hasCookie: boolean;
  source: string | null;
  status: "missing" | "verified" | "blocked" | string;
  updatedAt: string | null;
  message: string;
};

type AvatarRefreshSummary = {
  total: number;
  success: number;
  failed: number;
  lastError: string | null;
};

type DuplicateGroup = {
  key: string;
  count: number;
  totalSizeGb: number;
  files: LibraryFile[];
};

type NumericDuplicateGroup = {
  folder: string;
  number: string;
  count: number;
  files: LibraryFile[];
};

type DedupKind = "code" | "name" | "numeric";

type DedupDisplayGroup = {
  id: string;
  kind: DedupKind;
  key: string;
  title: string;
  subtitle: string;
  count: number;
  totalSizeGb: number;
  files: LibraryFile[];
  highlight: string;
  latestModifiedUnix: number;
};

type ScanReport = {
  stats: {
    rootPath: string;
    fileCount: number;
    folderCount: number;
    rootFileCount: number;
    videoCount: number;
    totalSizeGb: number;
    scannedAt: string;
  };
  actors: ActorSummary[];
  actorSnapshots?: unknown[];
  appreciationPath?: string;
  appreciationVideos: LibraryFile[];
  appreciationSnapshotUnix?: number;
  rootFiles: LibraryFile[];
  nameDuplicates: DuplicateGroup[];
  codeDuplicates: DuplicateGroup[];
  numericDuplicates: NumericDuplicateGroup[];
  downloadMovePlans: DownloadMovePlan[];
};

type PageId = "library" | "processing" | "archive" | "dedup";
type ActorSort = "modified" | "size" | "count" | "name";
type VideoSort = "modified" | "size" | "name" | "code";
type SortDirection = "desc" | "asc";

const DEFAULT_ROOT = "D:\\KawaLibrary";
const DOWNLOAD_FOLDER = "_downloading";
const APPRECIATION_FOLDER = "_appreciation";
const DEFAULT_METADATA_SITE = "www.javbus.com";
const DEFAULT_COVER_CAPTURE_PERCENT = 10;
const SETTINGS_STORAGE_KEY = "kawa.library.settings";
const DEDUP_IGNORE_STORAGE_KEY = "kawa.dedup.ignored";
const PROCESSING_OPTIONS_STORAGE_KEY = "kawa.processing.options";
const ACTOR_VIEW_STORAGE_KEY = "kawa.library.actorView";
const VIDEO_VIEW_STORAGE_KEY_PREFIX = "kawa.videoGrid";
const DEDUP_VIEW_STORAGE_KEY = "kawa.dedup.view";

const VIDEO_GRID_MIN_COLUMNS = 3;
const VIDEO_GRID_MIN_ROWS = 3;
const VIDEO_GRID_GAP = 12;
const VIDEO_META_HEIGHT = 58;
const VIDEO_MIN_COVER_HEIGHT = 64;
const VIDEO_MAX_COVER_HEIGHT = 170;
const COVER_REQUEST_CONCURRENCY = 2;

const defaultOptions: ProcessingOptions = {
  moveToAppreciation: false,
  renameEnabled: false,
  uppercase: false,
  normalizeDash: false,
  normalizeUncensoredSuffix: false,
  removeAtPrefix: false,
  skipIfExists: false,
  deleteSourceFolderAfterMove: false
};

const pages: Array<{ id: PageId; label: string; icon: React.ElementType }> = [
  { id: "library", label: "文件库", icon: Library },
  { id: "processing", label: "文件处理", icon: ArrowDownToLine },
  { id: "archive", label: "欣赏区归档", icon: FileVideo },
  { id: "dedup", label: "查重", icon: BadgeAlert }
];

function App() {
  const [settings, setSettings] = useState<LibrarySettings>(loadInitialSettings);
  const [settingsDraft, setSettingsDraft] = useState<LibrarySettings>(settings);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [activePage, setActivePage] = useState<PageId>("library");
  const [report, setReport] = useState<ScanReport | null>(null);
  const [downloadPlans, setDownloadPlans] = useState<DownloadMovePlan[]>([]);
  const [logs, setLogs] = useState<OperationLog[]>([]);
  const [actorProfiles, setActorProfiles] = useState<Record<string, ActorProfile>>({});
  const [metadataSession, setMetadataSession] = useState<MetadataSessionStatus | null>(null);
  const [selectedActorPath, setSelectedActorPath] = useState<string | null>(null);
  const [options, setOptions] = useState<ProcessingOptions>(loadStoredProcessingOptions);
  const [actorSort, setActorSort] = useState<ActorSort>(() => loadStoredActorView().sort);
  const [actorSortDirection, setActorSortDirection] = useState<SortDirection>(() => loadStoredActorView().direction);
  const [actorExpanded, setActorExpanded] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [isBooting, setIsBooting] = useState(true);
  const [status, setStatus] = useState("等待扫描");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    refreshLogs();
    refreshActorProfiles();
    void refreshMetadataSessionStatus(settings.metadataSiteUrl);
    void loadCachedOrScan(settings);
  }, []);

  useEffect(() => {
    void refreshMetadataSessionStatus(settings.metadataSiteUrl);
  }, [settings.metadataSiteUrl]);

  useEffect(() => {
    saveStoredProcessingOptions(options);
  }, [options]);

  useEffect(() => {
    saveStoredActorView({ sort: actorSort, direction: actorSortDirection });
  }, [actorSort, actorSortDirection]);

  function acceptReport(nextReport: ScanReport) {
    const normalized = normalizeReport(nextReport);
    setReport(normalized);
    setDownloadPlans(normalized.downloadMovePlans);
    setSelectedActorPath((current) => {
      if (current && normalized.actors.some((actor) => actor.path === current)) return current;
      return normalized.actors[0]?.path ?? null;
    });
  }

  async function refreshLogs() {
    try {
      setLogs(await invoke<OperationLog[]>("load_operation_logs"));
    } catch {
      setLogs([]);
    }
  }

  async function refreshActorProfiles() {
    try {
      const profiles = await invoke<ActorProfile[]>("load_actor_profiles");
      setActorProfiles(Object.fromEntries(profiles.map((profile) => [profile.actorPath, profile])));
    } catch {
      setActorProfiles({});
    }
  }

  async function refreshMetadataSessionStatus(siteUrl = settings.metadataSiteUrl) {
    try {
      const session = await invoke<MetadataSessionStatus>("load_metadata_session_status", {
        siteUrl
      });
      setMetadataSession(session);
    } catch {
      setMetadataSession(null);
    }
  }

  async function importMetadataSession(siteUrl: string, browser: LibrarySettings["metadataBrowser"]) {
    const session = await invoke<MetadataSessionStatus>("import_browser_metadata_session", {
      siteUrl,
      browser
    });
    setMetadataSession(session);
    return session;
  }

  async function saveManualMetadataSession(siteUrl: string, cookieHeader: string) {
    const session = await invoke<MetadataSessionStatus>("save_manual_metadata_session", {
      siteUrl,
      cookieHeader
    });
    setMetadataSession(session);
    return session;
  }

  async function clearMetadataSession(siteUrl: string) {
    await invoke("clear_metadata_session", { siteUrl });
    await refreshMetadataSessionStatus(siteUrl);
  }

  async function refreshActorAvatar(actor: ActorSummary) {
    if (!settings.onlineMetadataEnabled) return;
    try {
      const profile = await invoke<ActorProfile>("refresh_actor_avatar", {
        rootPath: settings.rootPath,
        actorPath: actor.path,
        siteUrl: settings.metadataSiteUrl
      });
      setActorProfiles((current) => ({ ...current, [profile.actorPath]: profile }));
    } catch (err) {
      setError(formatError(err));
      await refreshMetadataSessionStatus(settings.metadataSiteUrl);
    }
  }

  async function refreshAllActorAvatars() {
    const actors = report?.actors ?? [];
    if (actors.length === 0) {
      return { total: 0, success: 0, failed: 0, lastError: "当前没有可刷新的演员" } satisfies AvatarRefreshSummary;
    }
    if (!settings.onlineMetadataEnabled) {
      return { total: actors.length, success: 0, failed: actors.length, lastError: "请先启用联网获取" } satisfies AvatarRefreshSummary;
    }

    const updates: Record<string, ActorProfile> = {};
    let success = 0;
    let failed = 0;
    let lastError: string | null = null;

    for (const actor of actors) {
      try {
        const profile = await invoke<ActorProfile>("refresh_actor_avatar", {
          rootPath: settings.rootPath,
          actorPath: actor.path,
          siteUrl: settings.metadataSiteUrl
        });
        updates[profile.actorPath] = profile;
        success += 1;
      } catch (err) {
        failed += 1;
        lastError = formatError(err);
      }
    }

    if (Object.keys(updates).length > 0) {
      setActorProfiles((current) => ({ ...current, ...updates }));
    }
    await refreshMetadataSessionStatus(settings.metadataSiteUrl);
    if (lastError) {
      setError(lastError);
    }
    return { total: actors.length, success, failed, lastError } satisfies AvatarRefreshSummary;
  }

  async function chooseFolder(field: keyof LibrarySettings, defaultPath: string) {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath
      });
      if (typeof selected === "string") {
        setSettingsDraft((current) => ({
          ...current,
          [field]: selected,
          ...(field === "rootPath"
            ? {
                downloadPath: joinWinPath(selected, DOWNLOAD_FOLDER),
                appreciationPath: joinWinPath(selected, APPRECIATION_FOLDER)
              }
            : {})
        }));
      }
    } catch (err) {
      setError(formatError(err));
    }
  }

  function openSettings() {
    setSettingsDraft(settings);
    setSettingsOpen(true);
    void refreshMetadataSessionStatus(settings.metadataSiteUrl);
  }

  async function saveSettings() {
    setSettings(settingsDraft);
    saveStoredSettings(settingsDraft);
    setSettingsOpen(false);
    await refreshMetadataSessionStatus(settingsDraft.metadataSiteUrl);
    await scanWithSettings(settingsDraft);
  }

  async function scan() {
    await scanWithSettings(settings);
  }

  async function loadCachedOrScan(nextSettings: LibrarySettings) {
    setIsBusy(true);
    setIsBooting(true);
    setError(null);
    setStatus("正在加载缓存");
    try {
      const cachedReport = await invoke<ScanReport | null>("load_cached_library", {
        rootPath: nextSettings.rootPath,
        downloadPath: nextSettings.downloadPath,
        appreciationPath: nextSettings.appreciationPath,
        coverPercent: nextSettings.coverCapturePercent,
        options
      });
      if (cachedReport) {
        const normalized = normalizeReport(cachedReport);
        acceptReport(normalized);
        const plans = normalized.downloadMovePlans;
        setStatus(`已加载缓存：${plans.filter((plan) => plan.status === "ready").length} 项可执行`);
        return;
      }
      await scanWithSettings(nextSettings);
    } catch (err) {
      setError(formatError(err));
      setStatus("缓存加载失败");
    } finally {
      setIsBusy(false);
      setIsBooting(false);
    }
  }

  async function scanWithSettings(nextSettings: LibrarySettings) {
    setIsBusy(true);
    setError(null);
    setStatus("正在扫描文件库");
    try {
      const nextReport = await invoke<ScanReport>("scan_library", {
        rootPath: nextSettings.rootPath,
        downloadPath: nextSettings.downloadPath,
        appreciationPath: nextSettings.appreciationPath,
        coverPercent: nextSettings.coverCapturePercent,
        options
      });
      acceptReport(nextReport);
      const plans = nextReport.downloadMovePlans ?? [];
      setStatus(`扫描完成：${plans.filter((plan) => plan.status === "ready").length} 项可执行`);
    } catch (err) {
      setError(formatError(err));
      setStatus("扫描失败");
    } finally {
      setIsBusy(false);
      setIsBooting(false);
    }
  }

  async function generateProcessingPreview() {
    setIsBusy(true);
    setError(null);
    setStatus("正在刷新下载区处理");
    try {
      const plans = await loadProcessingPreview(settings, options);
      setDownloadPlans(plans);
      setStatus(`下载区处理已刷新：${plans.filter((plan) => plan.status === "ready").length} 项可执行`);
    } catch (err) {
      setError(formatError(err));
      setStatus("刷新下载区处理失败");
    } finally {
      setIsBusy(false);
    }
  }

  useEffect(() => {
    if (!report) return;
    const timer = window.setTimeout(() => {
      void generateProcessingPreview();
    }, 250);
    return () => window.clearTimeout(timer);
  }, [options]);

  async function loadProcessingPreview(nextSettings: LibrarySettings, nextOptions: ProcessingOptions) {
    return invoke<DownloadMovePlan[]>("preview_download_move_plan", {
      rootPath: nextSettings.rootPath,
      downloadPath: nextSettings.downloadPath,
      appreciationPath: nextSettings.appreciationPath,
      options: nextOptions
    });
  }

  async function executeToAppreciation() {
    const requests: DownloadProcessingRequest[] = downloadPlans
      .filter((plan) => plan.status === "ready" && plan.sourcePath && plan.targetPath)
      .map((plan) => ({
        id: plan.id ?? plan.sourcePath!,
        sourcePath: plan.sourcePath!,
        targetPath: plan.targetPath!,
        status: plan.status
      }));

    if (requests.length === 0) {
      setStatus("没有可执行到欣赏区的文件");
      return;
    }

    setIsBusy(true);
    setError(null);
    setStatus("正在执行到欣赏区");
    try {
      const result = await invoke<OperationResult>("execute_download_move_plan", {
        rootPath: settings.rootPath,
        downloadPath: settings.downloadPath,
        appreciationPath: settings.appreciationPath,
        requests,
        options
      });
      setStatus(`执行完成：成功 ${result.success}，跳过 ${result.skipped}，失败 ${result.failed}`);
      await refreshLogs();
      await scan();
    } catch (err) {
      setError(formatError(err));
      setStatus("执行失败");
    } finally {
      setIsBusy(false);
    }
  }

  async function openVideo(path: string) {
    try {
      await invoke("open_media_file", { rootPath: settings.rootPath, filePath: path });
    } catch (err) {
      setError(formatError(err));
    }
  }

  async function openFileLocation(path: string) {
    try {
      await invoke("open_file_location", { rootPath: settings.rootPath, filePath: path });
    } catch (err) {
      setError(formatError(err));
    }
  }

  async function openExternal(url: string) {
    try {
      await invoke("open_external_url", { url });
    } catch (err) {
      setError(formatError(err));
    }
  }

  const sortedActors = useMemo(() => sortActors(report?.actors ?? [], actorSort, actorSortDirection), [report, actorSort, actorSortDirection]);
  const selectedActor = useMemo(() => {
    if (!sortedActors.length) return null;
    return sortedActors.find((actor) => actor.path === selectedActorPath) ?? sortedActors[0];
  }, [sortedActors, selectedActorPath]);
  const readyCount = downloadPlans.filter((plan) => plan.status === "ready").length;
  const metadataReady = metadataSession?.status === "verified";
  const onlineMetadataActive = settings.onlineMetadataEnabled && metadataReady;

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">K</div>
        </div>

        <nav className="tab-list">
          {pages.map((page) => {
            const Icon = page.icon;
            return (
              <button
                key={page.id}
                className={activePage === page.id ? "tab active" : "tab"}
                onClick={() => setActivePage(page.id)}
                title={page.label}
              >
                <Icon size={18} />
              </button>
            );
          })}
        </nav>

        <button className="settings-entry" onClick={openSettings} title="设置">
          <Settings size={18} />
        </button>
      </aside>

      <section className="workspace">
        {report ? (
          activePage === "library" ? (
            <LibraryPage
              rootPath={settings.rootPath}
              coverCapturePercent={settings.coverCapturePercent}
              actors={sortedActors}
              selectedActor={selectedActor}
              sort={actorSort}
              expanded={actorExpanded}
              profiles={actorProfiles}
              avatarAutoEnabled={onlineMetadataActive}
              onSortChange={setActorSort}
              sortDirection={actorSortDirection}
              onSortDirectionChange={setActorSortDirection}
              onToggleExpanded={() => setActorExpanded((value) => !value)}
              onSelectActor={(path) => {
                setSelectedActorPath(path);
                setActorExpanded(false);
              }}
              isBusy={isBusy}
              onRefresh={scan}
              onOpenVideo={openVideo}
              onRefreshAvatar={refreshActorAvatar}
            />
          ) : activePage === "processing" ? (
            <ProcessingPage
              rootPath={settings.rootPath}
              coverCapturePercent={settings.coverCapturePercent}
              plans={downloadPlans}
              options={options}
              setOptions={setOptions}
              appreciationVideos={report.appreciationVideos}
              readyCount={readyCount}
              isBusy={isBusy}
              onRefreshPreview={generateProcessingPreview}
              onExecute={executeToAppreciation}
              onOpenVideo={openVideo}
            />
          ) : activePage === "archive" ? (
            <ArchivePage
              appreciationVideos={report.appreciationVideos}
              actors={sortedActors}
              profiles={actorProfiles}
              settings={settings}
              onlineMetadataActive={onlineMetadataActive}
              onOpenVideo={openVideo}
              onOpenExternal={openExternal}
              onArchived={async () => {
                await refreshLogs();
                await refreshActorProfiles();
                await scan();
              }}
            />
          ) : (
            <DedupPage report={report} onRefresh={scan} isBusy={isBusy} onOpenFileLocation={openFileLocation} />
          )
        ) : (
          <EmptyState onScan={scan} isBusy={isBusy} isBooting={isBooting} />
        )}
      </section>

      {settingsOpen ? (
        <SettingsDialog
          draft={settingsDraft}
          setDraft={setSettingsDraft}
          metadataSession={metadataSession}
          actorCount={report?.actors.length ?? 0}
          onChooseFolder={chooseFolder}
          onImportSession={importMetadataSession}
          onSaveManualSession={saveManualMetadataSession}
          onRefreshSessionStatus={refreshMetadataSessionStatus}
          onClearSession={clearMetadataSession}
          onOpenVerificationPage={openExternal}
          onRefreshAllAvatars={refreshAllActorAvatars}
          onClose={() => setSettingsOpen(false)}
          onSave={saveSettings}
        />
      ) : null}
    </main>
  );
}

function LibraryPage({
  rootPath,
  coverCapturePercent,
  actors,
  selectedActor,
  sort,
  sortDirection,
  expanded,
  profiles,
  avatarAutoEnabled,
  onSortChange,
  onSortDirectionChange,
  onToggleExpanded,
  onSelectActor,
  isBusy,
  onRefresh,
  onOpenVideo,
  onRefreshAvatar
}: {
  rootPath: string;
  coverCapturePercent: number;
  actors: ActorSummary[];
  selectedActor: ActorSummary | null;
  sort: ActorSort;
  sortDirection: SortDirection;
  expanded: boolean;
  profiles: Record<string, ActorProfile>;
  avatarAutoEnabled: boolean;
  onSortChange: (sort: ActorSort) => void;
  onSortDirectionChange: (direction: SortDirection) => void;
  onToggleExpanded: () => void;
  onSelectActor: (path: string) => void;
  isBusy: boolean;
  onRefresh: () => void;
  onOpenVideo: (path: string) => void;
  onRefreshAvatar: (actor: ActorSummary) => Promise<void>;
}) {
  const visibleActors = actors.slice(0, 12);
  const avatarRequestedRef = useRef(new Set<string>());
  const [avatarRefreshing, setAvatarRefreshing] = useState(false);

  useEffect(() => {
    if (!avatarAutoEnabled) return;
    const targets = [selectedActor, ...visibleActors].filter(Boolean) as ActorSummary[];
    for (const actor of targets) {
      const profile = profiles[actor.path];
      if (profile?.avatarPath || avatarRequestedRef.current.has(actor.path)) continue;
      avatarRequestedRef.current.add(actor.path);
      onRefreshAvatar(actor);
    }
  }, [avatarAutoEnabled, selectedActor?.path, visibleActors.map((actor) => actor.path).join("|"), profiles, onRefreshAvatar]);

  return (
    <div className="page-grid library-page">
      <section className="actor-list-panel">
        <div className="actor-list-head">
          <div>
            <h1>演员列表</h1>
          </div>
          <div className="library-tools">
            <SortSelect
              value={sort}
              direction={sortDirection}
              onChange={(value) => onSortChange(value as ActorSort)}
              onToggleDirection={() => onSortDirectionChange(sortDirection === "desc" ? "asc" : "desc")}
              options={[
                { label: "按时间排序", value: "modified" },
                { label: "按大小排序", value: "size" },
                { label: "按视频数量排序", value: "count" },
                { label: "按首字母排序", value: "name" }
              ]}
            />
            <button className="primary-button" onClick={onRefresh} disabled={isBusy}>
              {isBusy ? <Loader2 className="spin" size={18} /> : <RefreshCw size={18} />}
              <span>{isBusy ? "刷新中" : "刷新"}</span>
            </button>
            <button className="ghost-button" onClick={onToggleExpanded} disabled={actors.length === 0}>
              <ChevronDown className={expanded ? "chevron-up" : ""} size={16} />
              <span>{expanded ? "收起" : "展开"}</span>
            </button>
          </div>
        </div>

        <div className="avatar-strip single-row">
          {actors.length === 0 ? (
            <EmptyLine text="没有扫描到演员目录" />
          ) : (
            visibleActors.map((actor) => (
              <ActorAvatar
                key={actor.path}
                actor={actor}
                profile={profiles[actor.path]}
                active={selectedActor?.path === actor.path}
                onSelect={onSelectActor}
              />
            ))
          )}
        </div>
        {expanded && actors.length > visibleActors.length ? (
          <div className="avatar-expanded-grid">
            {actors.slice(visibleActors.length).map((actor) => (
              <ActorAvatar
                key={actor.path}
                actor={actor}
                profile={profiles[actor.path]}
                active={selectedActor?.path === actor.path}
                onSelect={onSelectActor}
              />
            ))}
          </div>
        ) : null}
      </section>

      {selectedActor ? (
        <section className="actor-detail-layout">
          <aside className="actor-panel">
            <div className="actor-photo" style={visualStyle(selectedActor.name)}>
              {profiles[selectedActor.path]?.avatarPath ? (
                <img src={convertFileSrc(profiles[selectedActor.path].avatarPath!)} alt="" />
              ) : (
                <UserRound size={58} />
              )}
            </div>
            <div className="actor-title with-action">
              <h2>{selectedActor.name}</h2>
              <button
                className="ghost-button"
                disabled={avatarRefreshing}
                onClick={async () => {
                  setAvatarRefreshing(true);
                  try {
                    await onRefreshAvatar(selectedActor);
                  } finally {
                    setAvatarRefreshing(false);
                  }
                }}
              >
                {avatarRefreshing ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
                <span>{avatarRefreshing ? "刷新中" : "刷新头像"}</span>
              </button>
            </div>
            <div className="pill-wrap">
              {selectedActor.roles.map((role) => (
                <span className="pill" key={role}>{role}</span>
              ))}
            </div>
            <div className="meta-list">
              <MetaItem label="已下载视频" value={`${selectedActor.videos.length} 部`} />
              <MetaItem label="文件数量" value={`${selectedActor.fileCount} 个`} />
              <MetaItem label="目录大小" value={`${formatNumber(selectedActor.totalSizeGb)} GB`} />
              <MetaItem label="最近更新" value={selectedActor.modified} />
            </div>
          </aside>

          <section className="video-panel">
            <div className="panel-heading with-action">
              <div>
                <div className="heading-title">
                  <FileVideo size={18} />
                  <h2>已下载视频</h2>
                </div>
              </div>
            </div>
            <VideoGrid
              rootPath={rootPath}
              coverCapturePercent={coverCapturePercent}
              videos={selectedActor.videos}
              storageKey={`library:${selectedActor.path}`}
              onOpenVideo={onOpenVideo}
            />
          </section>
        </section>
      ) : null}
    </div>
  );
}

function ActorAvatar({
  actor,
  profile,
  active,
  onSelect
}: {
  actor: ActorSummary;
  profile?: ActorProfile;
  active: boolean;
  onSelect: (path: string) => void;
}) {
  return (
    <button
      className={active ? "avatar-chip active" : "avatar-chip"}
      onClick={() => onSelect(actor.path)}
      title={displayPath(actor.path)}
    >
      <div className="avatar-face" style={visualStyle(actor.name)}>
        {profile?.avatarPath ? (
          <img src={convertFileSrc(profile.avatarPath)} alt="" />
        ) : (
          <span>{actor.name.slice(0, 1).toUpperCase()}</span>
        )}
      </div>
      <span className="avatar-name">{actor.name}</span>
    </button>
  );
}

function ProcessingPage({
  rootPath,
  coverCapturePercent,
  plans,
  options,
  setOptions,
  appreciationVideos,
  readyCount,
  isBusy,
  onRefreshPreview,
  onExecute,
  onOpenVideo
}: {
  rootPath: string;
  coverCapturePercent: number;
  plans: DownloadMovePlan[];
  options: ProcessingOptions;
  setOptions: (options: ProcessingOptions) => void;
  appreciationVideos: LibraryFile[];
  readyCount: number;
  isBusy: boolean;
  onRefreshPreview: () => void;
  onExecute: () => void;
  onOpenVideo: (path: string) => void;
}) {
  function updateOption<K extends keyof ProcessingOptions>(key: K, value: ProcessingOptions[K]) {
    setOptions({ ...options, [key]: value });
  }

  return (
    <div className="page-grid processing-page">
      <section className="process-layout">
        <div className="process-main-column">
          <section className="panel">
            <div className="panel-heading with-action">
              <div className="heading-title">
                <ArrowDownToLine size={18} />
                <h2>下载区处理</h2>
              </div>
              <button className="ghost-button small" onClick={onRefreshPreview} disabled={isBusy}>
                {isBusy ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
                <span>刷新</span>
              </button>
            </div>
            <div className="table pending-table">
              <div className="table-head pending-grid compact">
                <span>下载项</span>
                <span>处理后文件名</span>
                <span>说明</span>
              </div>
              {plans.length === 0 ? (
                <EmptyLine text="下载区没有待处理视频" />
              ) : (
                plans.map((plan) => (
                  <div className="table-row pending-grid compact" key={plan.id ?? plan.folderPath}>
                    <FileCell title={downloadItemTitle(plan)} subtitle={downloadItemSubtitle(plan)} />
                    <FileCell title={plan.targetFileName || "跳过"} />
                    <span className={plan.status === "ready" ? "table-note good" : "table-note"}>{plan.reason}</span>
                  </div>
                ))
              )}
            </div>
          </section>

          <section className="panel">
            <div className="panel-heading with-action">
              <div>
                <div className="heading-title">
                  <PlayCircle size={18} />
                  <h2>欣赏区</h2>
                </div>
              </div>
              <span className="pill warn">{appreciationVideos.length} 部</span>
            </div>
            <VideoGrid
              rootPath={rootPath}
              coverCapturePercent={coverCapturePercent}
              videos={appreciationVideos.map(libraryFileToActorVideo)}
              storageKey="appreciation"
              onOpenVideo={onOpenVideo}
            />
          </section>
        </div>

        <aside className="panel options-panel">
          <div className="panel-heading process-options-title">
            <div className="heading-title">
              <Settings2 size={18} />
              <h2>处理选项</h2>
            </div>
            <span className="pill warn">{readyCount} 项可执行</span>
          </div>

          <div className="option-list">
            <OptionRow checked={options.moveToAppreciation} label="移动到欣赏区" onChange={(checked) => updateOption("moveToAppreciation", checked)} />
            <OptionRow checked={options.renameEnabled} label="执行重命名" onChange={(checked) => updateOption("renameEnabled", checked)} />
            <OptionRow checked={options.uppercase} label="自动大写文件名" onChange={(checked) => updateOption("uppercase", checked)} />
            <OptionRow checked={options.normalizeDash} label="自动补番号横杠" onChange={(checked) => updateOption("normalizeDash", checked)} />
            <OptionRow checked={options.normalizeUncensoredSuffix} label="去码词改为 -U / -UC" onChange={(checked) => updateOption("normalizeUncensoredSuffix", checked)} />
            <OptionRow checked={options.removeAtPrefix} label="去掉 @ 前缀" onChange={(checked) => updateOption("removeAtPrefix", checked)} />
            <OptionRow checked={options.skipIfExists} label="目标已存在时跳过" onChange={(checked) => updateOption("skipIfExists", checked)} />
            <OptionRow
              checked={options.deleteSourceFolderAfterMove}
              label="移动完成后删除下载文件夹"
              onChange={(checked) => updateOption("deleteSourceFolderAfterMove", checked)}
            />
          </div>
          <div className="action-stack options-actions">
            <button className="danger-button" onClick={onExecute} disabled={isBusy || readyCount === 0}>
              <ArrowDownToLine size={17} />
              <span>执行到欣赏区</span>
            </button>
          </div>
        </aside>
      </section>
    </div>
  );
}

function DedupPage({
  report,
  isBusy,
  onRefresh,
  onOpenFileLocation
}: {
  report: ScanReport;
  isBusy: boolean;
  onRefresh: () => void;
  onOpenFileLocation: (path: string) => void;
}) {
  const [showMultipartGroups, setShowMultipartGroups] = useState(() => loadStoredDedupView().showMultipartGroups);
  const [showIgnoredGroups, setShowIgnoredGroups] = useState(() => loadStoredDedupView().showIgnoredGroups);
  const ignoreStorageKey = `${DEDUP_IGNORE_STORAGE_KEY}:${displayPath(report.stats.rootPath)}`;
  const [ignoredAtByKey, setIgnoredAtByKey] = useState<Record<string, number>>({});

  useEffect(() => {
    setIgnoredAtByKey(loadDedupIgnoreMap(ignoreStorageKey));
  }, [ignoreStorageKey]);

  useEffect(() => {
    saveStoredDedupView({ showMultipartGroups, showIgnoredGroups });
  }, [showMultipartGroups, showIgnoredGroups]);

  const allCodeGroups = useMemo(
    () => (report.codeDuplicates ?? []).map((group) => duplicateGroupToDisplay(group, "code", "相同番号")).sort(sortDedupDisplayGroups),
    [report.codeDuplicates]
  );
  const multipartGroups = useMemo(() => allCodeGroups.filter(isMultipartSuffixDedupGroup), [allCodeGroups]);
  const candidateGroups = useMemo(
    () => (showMultipartGroups ? allCodeGroups : allCodeGroups.filter((group) => !isMultipartSuffixDedupGroup(group))),
    [allCodeGroups, showMultipartGroups]
  );
  const ignoredGroups = useMemo(
    () => candidateGroups.filter((group) => isIgnoredDedupGroup(group, ignoredAtByKey)),
    [candidateGroups, ignoredAtByKey]
  );
  const visibleGroups = showIgnoredGroups ? candidateGroups : candidateGroups.filter((group) => !isIgnoredDedupGroup(group, ignoredAtByKey));
  const duplicateFiles = useMemo(() => uniqueFilesFromDedupGroups(visibleGroups), [visibleGroups]);
  const hiddenMultipartCount = allCodeGroups.length - (showMultipartGroups ? allCodeGroups.length : candidateGroups.length);
  const ignoredGroupCount = ignoredGroups.length;

  function ignoreGroup(group: DedupDisplayGroup) {
    const next = { ...ignoredAtByKey, [group.key]: Date.now() };
    setIgnoredAtByKey(next);
    saveDedupIgnoreMap(ignoreStorageKey, next);
  }

  return (
    <div className="page-grid dedup-page">
      <section className="panel dedup-panel">
        <div className="panel-heading with-action">
          <div>
            <div className="heading-title">
              <BadgeAlert size={18} />
              <h2>查重</h2>
            </div>
            <p>只按番号统计重复文件。分集后缀 1/2/3 默认不算重复，可手动显示。</p>
          </div>
          <button className="primary-button" onClick={onRefresh} disabled={isBusy}>
            {isBusy ? <Loader2 className="spin" size={18} /> : <RefreshCw size={18} />}
            <span>{isBusy ? "扫描中" : "重新扫描"}</span>
          </button>
        </div>

        <div className="dedup-stats">
          <MetricCard label="番号重复" value={visibleGroups.length} tone={visibleGroups.length ? "warn" : undefined} />
          <MetricCard label="涉及文件" value={duplicateFiles.length} tone={duplicateFiles.length ? "warn" : undefined} />
          <MetricCard label="已忽略" value={ignoredGroupCount} />
          <MetricCard label="隐藏分集" value={hiddenMultipartCount} />
        </div>

        <div className="dedup-toolbar">
          <OptionRow
            checked={showMultipartGroups}
            label="显示 1/2/3 分集后缀的番号重复"
            onChange={setShowMultipartGroups}
          />
          <OptionRow
            checked={showIgnoredGroups}
            label="显示已忽略"
            onChange={setShowIgnoredGroups}
          />
          <span>{showMultipartGroups ? "当前包含分集文件" : `已隐藏 ${hiddenMultipartCount} 组分集文件`}</span>
        </div>

        <div className="dedup-result-area">
          {allCodeGroups.length === 0 ? (
            <div className="dedup-empty">
              <CheckCircle2 size={30} />
              <strong>当前没有发现番号重复</strong>
              <span>如果刚整理过文件，可以点右上角重新扫描同步最新结果。</span>
            </div>
          ) : visibleGroups.length === 0 ? (
            <div className="dedup-empty">
              <CheckCircle2 size={30} />
              <strong>当前可疑重复已全部隐藏</strong>
              <span>这些重复都是 1/2/3 分集后缀。勾选上方选项可以查看。</span>
            </div>
          ) : (
            <div className="dedup-code-list">
              {visibleGroups.map((group) => (
                <DedupResultCard
                  group={group}
                  ignoredAt={ignoredAtByKey[group.key]}
                  key={group.id}
                  onIgnore={ignoreGroup}
                  onOpenFileLocation={onOpenFileLocation}
                />
              ))}
            </div>
          )}
        </div>
      </section>

      <section className="panel dedup-side-panel">
        <div className="panel-heading">
          <div className="heading-title">
            <Search size={18} />
            <h2>番号列表</h2>
          </div>
        </div>
        <div className="dedup-side-summary">
          <div>
            <span>重复组</span>
            <strong>{visibleGroups.length}</strong>
          </div>
          <div>
            <span>涉及容量</span>
            <strong>{formatNumber(sumFileSizeGb(duplicateFiles))} GB</strong>
          </div>
        </div>
        <DedupMiniList title="番号重复" groups={visibleGroups} emptyText="没有发现番号重复" />
        {multipartGroups.length > 0 ? (
          <DedupMiniList title={showMultipartGroups ? "分集后缀" : "已隐藏分集"} groups={multipartGroups} emptyText="没有隐藏分集" />
        ) : null}
        <div className="muted-box">
          文件名重复和番号重复本质上都在判断同一作品，这里统一只看番号。带 1/2/3 后缀的分集默认视为正常拆分。
        </div>
      </section>
    </div>
  );
}

function ArchivePage({
  appreciationVideos,
  actors,
  profiles,
  settings,
  onlineMetadataActive,
  onOpenVideo,
  onOpenExternal,
  onArchived
}: {
  appreciationVideos: LibraryFile[];
  actors: ActorSummary[];
  profiles: Record<string, ActorProfile>;
  settings: LibrarySettings;
  onlineMetadataActive: boolean;
  onOpenVideo: (path: string) => void;
  onOpenExternal: (url: string) => void | Promise<void>;
  onArchived: () => void | Promise<void>;
}) {
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [lookups, setLookups] = useState<Record<string, ArchiveLookup>>({});
  const [searchingCodes, setSearchingCodes] = useState<Set<string>>(new Set());
  const [selectedActorPath, setSelectedActorPath] = useState<string | null>(null);
  const [movePreview, setMovePreview] = useState<ArchiveMovePreview | null>(null);
  const [archiveBusy, setArchiveBusy] = useState(false);
  const [archiveMessage, setArchiveMessage] = useState("");
  const [specificActorOpen, setSpecificActorOpen] = useState(false);
  const [specificActorSort, setSpecificActorSort] = useState<ActorSort>("name");
  const [specificActorSortDirection, setSpecificActorSortDirection] = useState<SortDirection>("asc");
  const loadedCodesRef = useRef(new Set<string>());
  const autoSearchRef = useRef(new Set<string>());
  const videos = useMemo(() => appreciationVideos.map(libraryFileToActorVideo), [appreciationVideos]);
  const selectedVideo = videos.find((video) => video.path === selectedPath) ?? videos[0] ?? null;
  const normalizedCode = selectedVideo ? normalizeArchiveCode(selectedVideo.name) : "";
  const isFc2Code = isFc2ArchiveCode(normalizedCode);
  const lookup = normalizedCode ? lookups[normalizedCode] : undefined;
  const matchedActors = useMemo(
    () => (normalizedCode ? matchActorsByCode(actors, normalizedCode) : []),
    [actors, normalizedCode]
  );
  const candidates = useMemo(() => {
    const byPath = new Map<string, ActorSummary>();
    const lookupActor = lookup?.actorPath ? actors.find((actor) => actor.path === lookup.actorPath) : undefined;
    if (lookupActor) byPath.set(lookupActor.path, lookupActor);
    for (const actor of matchedActors) byPath.set(actor.path, actor);
    return [...byPath.values()];
  }, [actors, lookup?.actorPath, matchedActors]);
  const effectiveActorPath = selectedActorPath ?? candidates[0]?.path ?? null;
  const effectiveActor = effectiveActorPath ? actors.find((actor) => actor.path === effectiveActorPath) ?? null : null;
  const searchUrl = normalizedCode && !isFc2Code ? buildMetadataSearchUrl(settings.metadataSiteUrl, normalizedCode) : "";
  const archiveCodes = useMemo(() => uniqueArchiveCodes(videos), [videos]);
  const archiveCodesKey = archiveCodes.join("|");
  const searchedCount = archiveCodes.filter((code) => {
    const entry = lookups[code];
    return entry && !needsArchiveLookupRefresh(entry);
  }).length;
  const unsearchedCount = archiveCodes.length - searchedCount;
  const isLookupInvalid = lookup ? needsArchiveLookupRefresh(lookup) : false;
  const lookupActorExists = Boolean(lookup?.actorPath && actors.some((actor) => actor.path === lookup.actorPath));
  const createActorName = !isLookupInvalid && lookup?.actorName && !lookupActorExists ? lookup.actorName : "";
  const specificActors = useMemo(
    () => sortActors(actors, specificActorSort, specificActorSortDirection),
    [actors, specificActorSort, specificActorSortDirection]
  );
  const canArchiveExistingActor = Boolean(effectiveActorPath && movePreview);
  const canArchiveNewActor = Boolean(createActorName);

  useEffect(() => {
    if (videos.length === 0) {
      setSelectedPath(null);
      return;
    }
    if (!selectedPath || !videos.some((video) => video.path === selectedPath)) {
      setSelectedPath(videos[0].path);
    }
  }, [selectedPath, videos]);

  useEffect(() => {
    setSelectedActorPath(null);
    setMovePreview(null);
    setSpecificActorOpen(false);
  }, [selectedPath, normalizedCode]);

  useEffect(() => {
    if (archiveCodes.length === 0) return;
    let cancelled = false;
    for (const code of archiveCodes) {
      if (loadedCodesRef.current.has(code)) continue;
      loadedCodesRef.current.add(code);
      void invoke<ArchiveLookup | null>("get_archive_lookup", { code }).then((result) => {
        if (cancelled || !result) return;
        setLookups((current) => ({ ...current, [result.code]: result }));
      }).catch(() => undefined);
    }
    return () => {
      cancelled = true;
    };
  }, [archiveCodesKey]);

  useEffect(() => {
    if (archiveCodes.length === 0) return;
    let cancelled = false;
    const pending = archiveCodes.filter((code) => {
      if (!onlineMetadataActive && !isFc2ArchiveCode(code)) return false;
      const lookup = lookups[code];
      return (!lookup || needsArchiveLookupRefresh(lookup)) && !autoSearchRef.current.has(code);
    });
    if (pending.length === 0) return;
    void (async () => {
      for (const code of pending) {
        if (cancelled) break;
        autoSearchRef.current.add(code);
        await searchCode(code);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [onlineMetadataActive, settings.metadataSiteUrl, archiveCodesKey, lookups]);

  useEffect(() => {
    if (!selectedVideo || !effectiveActorPath) {
      setMovePreview(null);
      setSpecificActorOpen(false);
      return;
    }
    let cancelled = false;
    void invoke<ArchiveMovePreview>("preview_archive_move", {
      rootPath: settings.rootPath,
      appreciationPath: settings.appreciationPath,
      videoPath: selectedVideo.path,
      actorPath: effectiveActorPath
    }).then((preview) => {
      if (!cancelled) setMovePreview(preview);
    }).catch((err) => {
      if (!cancelled) {
        setMovePreview({
          sourcePath: selectedVideo.path,
          targetPath: "",
          actorName: effectiveActor?.name ?? "",
          status: "conflict",
          message: formatError(err)
        });
      }
    });
    return () => {
      cancelled = true;
    };
  }, [selectedVideo?.path, effectiveActorPath, settings.rootPath, settings.appreciationPath]);

  async function searchCode(code: string) {
    if (!code) return;
    setSearchingCodes((current) => new Set(current).add(code));
    try {
      const result = await invoke<ArchiveLookup>("search_archive_metadata", {
        rootPath: settings.rootPath,
        code,
        siteUrl: settings.metadataSiteUrl
      });
      setLookups((current) => ({ ...current, [result.code]: result }));
      if (result.actorPath) setSelectedActorPath(result.actorPath);
      setArchiveMessage("");
    } catch (err) {
      setArchiveMessage(formatError(err));
    } finally {
      setSearchingCodes((current) => {
        const next = new Set(current);
        next.delete(code);
        return next;
      });
    }
  }

  function operationSummary(label: string, result: OperationResult) {
    return `${label}：成功 ${result.success}，跳过 ${result.skipped}，失败 ${result.failed}`;
  }

  function operationIssueMessage(result: OperationResult, fallback: string) {
    const issue = result.logs?.find((log) => log.status !== "success") ?? (result.success === 0 ? result.logs?.[0] : undefined);
    if (!issue) return "";
    return [issue.message || fallback, issue.targetPath ? `目标：${displayPath(issue.targetPath)}` : ""].filter(Boolean).join("\n");
  }

  function showOperationIssue(result: OperationResult, fallback: string) {
    if (result.skipped === 0 && result.failed === 0 && result.success > 0) return;
    const message = operationIssueMessage(result, fallback) || fallback;
    window.alert(message);
  }

  function previewConflictMessage(preview: ArchiveMovePreview) {
    return [preview.message || "目标文件存在冲突，已停止归档", preview.targetPath ? `目标：${displayPath(preview.targetPath)}` : ""].filter(Boolean).join("\n");
  }

  async function moveToActor() {
    if (!selectedVideo) return;
    if (movePreview && movePreview.status !== "ready") {
      const message = previewConflictMessage(movePreview);
      setArchiveMessage(message);
      window.alert(message);
      return;
    }
    if (effectiveActorPath && movePreview?.status === "ready") {
      setArchiveBusy(true);
      try {
        const result = await invoke<OperationResult>("execute_archive_move", {
          rootPath: settings.rootPath,
          appreciationPath: settings.appreciationPath,
          videoPath: selectedVideo.path,
          actorPath: effectiveActorPath
        });
        setArchiveMessage(operationSummary("归档完成", result));
        showOperationIssue(result, "归档失败");
        if (result.success > 0) await onArchived();
      } catch (err) {
        const message = formatError(err);
        setArchiveMessage(message);
        window.alert(message);
      } finally {
        setArchiveBusy(false);
      }
      return;
    }
    if (createActorName) {
      await moveToNewActor();
    }
  }

  async function moveToSpecificActor(actor: ActorSummary) {
    if (!selectedVideo) return;
    setArchiveBusy(true);
    try {
      const result = await invoke<OperationResult>("execute_archive_move", {
        rootPath: settings.rootPath,
        appreciationPath: settings.appreciationPath,
        videoPath: selectedVideo.path,
        actorPath: actor.path
      });
      setArchiveMessage(operationSummary(`已移动到 ${actor.name}`, result));
      showOperationIssue(result, `移动到 ${actor.name} 失败`);
      if (result.success > 0) {
        setSelectedActorPath(actor.path);
        setSpecificActorOpen(false);
        await onArchived();
      }
    } catch (err) {
      const message = formatError(err);
      setArchiveMessage(message);
      window.alert(message);
    } finally {
      setArchiveBusy(false);
    }
  }
  async function moveToNewActor() {
    if (!selectedVideo || !createActorName) return;
    setArchiveBusy(true);
    try {
      const result = await invoke<OperationResult>("execute_archive_move_to_new_actor", {
        rootPath: settings.rootPath,
        appreciationPath: settings.appreciationPath,
        videoPath: selectedVideo.path,
        actorName: createActorName
      });
      setArchiveMessage(operationSummary(`新建 ${createActorName} 文件夹并归档完成`, result));
      showOperationIssue(result, `新建 ${createActorName} 文件夹并归档失败`);
      if (result.success > 0) await onArchived();
    } catch (err) {
      const message = formatError(err);
      setArchiveMessage(message);
      window.alert(message);
    } finally {
      setArchiveBusy(false);
    }
  }

  async function deleteArchiveVideo() {
    if (!selectedVideo) return;
    const confirmed = window.confirm(`确定删除「${selectedVideo.name}」吗？此操作不可恢复。`);
    if (!confirmed) return;
    setArchiveBusy(true);
    try {
      const result = await invoke<OperationResult>("delete_archive_video", {
        rootPath: settings.rootPath,
        appreciationPath: settings.appreciationPath,
        videoPath: selectedVideo.path
      });
      setArchiveMessage(`删除完成：成功 ${result.success}，跳过 ${result.skipped}，失败 ${result.failed}`);
      await onArchived();
    } catch (err) {
      setArchiveMessage(formatError(err));
    } finally {
      setArchiveBusy(false);
    }
  }

  return (
    <div className="page-grid archive-page">
      <section className="panel archive-list-panel">
        <div className="panel-heading with-action">
          <div>
            <div className="heading-title">
              <FileVideo size={18} />
              <h2>欣赏区归档</h2>
            </div>
          </div>
          <div className="archive-summary">
            <span className="pill warn">{videos.length} 部待归档</span>
            <span className="pill good">{searchedCount} 已搜索</span>
            <span className="pill">{unsearchedCount} 未搜索</span>
          </div>
        </div>

        <div className="archive-video-list">
          {videos.length === 0 ? (
            <EmptyLine text="欣赏区没有待归档视频" />
          ) : (
            videos.map((video) => {
              const code = normalizeArchiveCode(video.name);
              return (
                <button
                  className={selectedVideo?.path === video.path ? "archive-video-row active" : "archive-video-row"}
                  key={video.path}
                  onClick={() => setSelectedPath(video.path)}
                >
                  <div>
                    <strong title={video.name}>{video.name}</strong>
                    <span>{code || video.name}</span>
                  </div>
                  <ArchiveStatusPill lookup={code ? lookups[code] : undefined} searching={code ? searchingCodes.has(code) : false} />
                </button>
              );
            })
          )}
        </div>
      </section>

      <aside className="panel archive-side-panel">
        <div className="panel-heading archive-side-top">
          <div className="heading-title">
            <Search size={18} />
            <h2>归档判断</h2>
          </div>
          <span className={settings.onlineMetadataEnabled ? "pill good" : "pill"}>{settings.onlineMetadataEnabled ? "联网启用" : "联网关闭"}</span>
        </div>

        {selectedVideo ? (
          <>
            <div className="archive-cover" onClick={() => onOpenVideo(selectedVideo.path)} role="button" tabIndex={0}>
              <VideoCover
                rootPath={settings.rootPath}
                coverCapturePercent={settings.coverCapturePercent}
                scrollRoot={null}
                video={selectedVideo}
                eager
              />
            </div>
            <div className="meta-list">
              <MetaItem label="识别番号" value={lookup?.code || normalizedCode || selectedVideo.name} />
              <MetaItem label="搜索状态" value={archiveLookupText(lookup, searchingCodes.has(normalizedCode))} />
              <MetaItem label="判断演员" value={isLookupInvalid ? "待重试" : lookup?.actorName ?? effectiveActor?.name ?? "未判断"} />
              <MetaItem label="搜索站点" value={settings.metadataSiteUrl || DEFAULT_METADATA_SITE} />
              <MetaItem label="本地候选演员" value={`${matchedActors.length} 个`} />
            </div>
            <div className="archive-candidate-list">
              {candidates.length === 0 ? (
                <EmptyLine text={!isLookupInvalid && lookup?.actorName ? "联网已识别演员，但本地没有同名目录" : "本地库没有匹配到相同番号的演员"} />
              ) : (
                candidates.map((actor) => (
                  <button
                    className={effectiveActorPath === actor.path ? "archive-candidate active" : "archive-candidate"}
                    key={actor.path}
                    onClick={() => setSelectedActorPath(actor.path)}
                  >
                    <strong>{actor.name}</strong>
                    <span>{displayPath(actor.path)}</span>
                  </button>
                ))
              )}
            </div>
            {movePreview ? <div className="muted-box">{movePreview.message}</div> : null}
            {archiveMessage ? <div className="muted-box">{archiveMessage}</div> : null}
            <div className="action-stack archive-actions">
              <button className="primary-button" disabled={(!settings.onlineMetadataEnabled && !isFc2Code) || !normalizedCode || searchingCodes.has(normalizedCode)} onClick={() => searchCode(normalizedCode)}>
                <Search size={17} />
                <span>{searchingCodes.has(normalizedCode) ? "搜索中" : isFc2Code ? "判断 FC2" : "搜索演员"}</span>
              </button>
              <button className="primary-button" disabled={archiveBusy || (!canArchiveExistingActor && !canArchiveNewActor)} onClick={moveToActor}>
                <CheckCircle2 size={17} />
                <span>{canArchiveNewActor && !effectiveActorPath ? `新建 ${createActorName} 文件夹并归档` : "确认归档到演员目录"}</span>
              </button>
              <button className="ghost-button" disabled={archiveBusy || actors.length === 0} onClick={() => setSpecificActorOpen((value) => !value)}>
                <UserRound size={17} />
                <span>移动到指定演员</span>
              </button>
              {specificActorOpen ? (
                <div className="archive-actor-picker">
                  <div className="archive-actor-picker-head">
                    <span>选择演员</span>
                    <SortSelect
                      value={specificActorSort}
                      direction={specificActorSortDirection}
                      onChange={(value) => setSpecificActorSort(value as ActorSort)}
                      onToggleDirection={() => setSpecificActorSortDirection((current) => (current === "desc" ? "asc" : "desc"))}
                      options={[
                        { label: "按首字母排序", value: "name" },
                        { label: "按时间排序", value: "modified" },
                        { label: "按大小排序", value: "size" },
                        { label: "按视频数量排序", value: "count" }
                      ]}
                    />
                  </div>
                  {actors.length === 0 ? (
                    <EmptyLine text="演员列表为空" />
                  ) : (
                    specificActors.map((actor) => {
                      const profile = profiles[actor.path];
                      return (
                        <button
                          className={selectedActorPath === actor.path ? "archive-actor-choice active" : "archive-actor-choice"}
                          disabled={archiveBusy}
                          key={actor.path}
                          onClick={() => moveToSpecificActor(actor)}
                          title={displayPath(actor.path)}
                        >
                          <div className="avatar-face" style={visualStyle(actor.name)}>
                            {profile?.avatarPath ? (
                              <img src={convertFileSrc(profile.avatarPath)} alt="" />
                            ) : (
                              <span>{actor.name.slice(0, 1).toUpperCase()}</span>
                            )}
                          </div>
                          <span>{actor.name}</span>
                        </button>
                      );
                    })
                  )}
                </div>
              ) : null}
              {createActorName ? (
                <button className="ghost-button" disabled={archiveBusy} onClick={moveToNewActor}>
                  <FolderPlus size={17} />
                  <span>{`新建 ${createActorName} 文件夹并归档`}</span>
                </button>
              ) : null}
              <button className="ghost-button" disabled={!searchUrl} onClick={() => searchUrl && onOpenExternal(searchUrl)}>
                <span>打开搜索页</span>
              </button>
              <button className="danger-button" disabled={archiveBusy} onClick={deleteArchiveVideo}>
                <Trash2 size={17} />
                <span>删除视频</span>
              </button>
            </div>
          </>
        ) : (
          <EmptyLine text="请选择欣赏区视频" />
        )}
      </aside>
    </div>
  );
}

function ArchiveStatusPill({ lookup, searching }: { lookup?: ArchiveLookup; searching: boolean }) {
  if (searching) return <span className="pill warn">搜索中</span>;
  if (!lookup || needsArchiveLookupRefresh(lookup)) return <span className="pill">未搜索</span>;
  return <span className={lookup.actorName ? "pill good" : "pill warn"}>{lookup.actorName ? `已搜索：${lookup.actorName}` : "已搜索：未匹配"}</span>;
}

function archiveLookupText(lookup: ArchiveLookup | undefined, searching: boolean) {
  if (searching) return "搜索中";
  if (!lookup || needsArchiveLookupRefresh(lookup)) return "未搜索";
  return lookup.actorName ? `已搜索（${lookup.actorName}）` : "已搜索（未匹配）";
}

function needsArchiveLookupRefresh(lookup: ArchiveLookup) {
  const actorName = lookup.actorName?.trim() ?? "";
  if (!actorName) return true;
  const lower = actorName.toLowerCase();
  return actorName === "0" || lower.includes("function") || lower.includes("searchs(") || lower.includes("ajax");
}

function MetricCard({ label, value, tone }: { label: string; value: number; tone?: "warn" }) {
  return (
    <div className={tone ? `metric ${tone}` : "metric"}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function DedupResultCard({
  group,
  ignoredAt,
  onIgnore,
  onOpenFileLocation
}: {
  group: DedupDisplayGroup;
  ignoredAt: number | undefined;
  onIgnore: (group: DedupDisplayGroup) => void;
  onOpenFileLocation: (path: string) => void;
}) {
  const isMultipart = isMultipartSuffixDedupGroup(group);
  const isIgnored = typeof ignoredAt === "number" && group.latestModifiedUnix * 1000 <= ignoredAt;
  return (
    <div className={isIgnored ? "dedup-code-group ignored" : "dedup-code-group"}>
      <div className="dedup-code-group-head">
        <div>
          <div className="dedup-title-line">
            <span className={isMultipart ? "pill" : "pill warn"}>{isMultipart ? "分集" : "重复"}</span>
            <h3 title={group.title}>{group.title}</h3>
          </div>
          <span title={displayPath(group.subtitle)}>{displayPath(group.subtitle)}</span>
        </div>
        <div className="dedup-group-actions">
          {isIgnored ? <span className="pill">已忽略</span> : null}
          <button className="ghost-button small" type="button" onClick={() => onIgnore(group)}>
            <span>{isIgnored ? "更新忽略" : "忽略"}</span>
          </button>
          <span className="pill warn">{group.count} 个文件 · {formatNumber(group.totalSizeGb)} GB</span>
        </div>
      </div>
      <div className="dup-files dedup-card-files">
        {group.files.length === 0 ? (
          <EmptyLine text="这组没有可展示的文件明细" />
        ) : (
          group.files.map((file, index) => (
            <button
              className="dup-file dedup-file-row dedup-file-button"
              key={file.path}
              type="button"
              onClick={() => onOpenFileLocation(file.path)}
              title={`打开所在位置：${displayPath(file.path)}`}
            >
              <span className="num-badge">{index + 1}</span>
              <div className="dedup-file-main">
                <strong title={file.name}>{highlightText(file.name, group.highlight)}</strong>
                <small title={displayPath(file.path)}>{displayPath(parentPath(file.path))}</small>
              </div>
              <div className="dedup-file-meta">
                <span>{formatNumber(file.sizeGb)} GB</span>
                <span>{file.modified}</span>
              </div>
            </button>
          ))
        )}
      </div>
    </div>
  );
}

function DedupMiniList({
  title,
  groups,
  emptyText
}: {
  title: string;
  groups: DedupDisplayGroup[];
  emptyText: string;
}) {
  return (
    <div className="dedup-mini-list">
      <div className="mini-log-head">
        <strong>{title}</strong>
        <span>{groups.length}</span>
      </div>
      {groups.length === 0 ? (
        <EmptyLine text={emptyText} />
      ) : (
        groups.slice(0, 8).map((group) => (
          <div className="dedup-mini-item" key={group.id}>
            <code title={group.title}>{group.title}</code>
            <span>{group.count} 个</span>
          </div>
        ))
      )}
      {groups.length > 8 ? <div className="dedup-more">还有 {groups.length - 8} 组未显示</div> : null}
    </div>
  );
}

function SettingsDialog({
  draft,
  setDraft,
  metadataSession,
  actorCount,
  onChooseFolder,
  onImportSession,
  onSaveManualSession,
  onRefreshSessionStatus,
  onClearSession,
  onOpenVerificationPage,
  onRefreshAllAvatars,
  onClose,
  onSave
}: {
  draft: LibrarySettings;
  setDraft: (settings: LibrarySettings) => void;
  metadataSession: MetadataSessionStatus | null;
  actorCount: number;
  onChooseFolder: (field: keyof LibrarySettings, defaultPath: string) => void;
  onImportSession: (siteUrl: string, browser: LibrarySettings["metadataBrowser"]) => Promise<MetadataSessionStatus>;
  onSaveManualSession: (siteUrl: string, cookieHeader: string) => Promise<MetadataSessionStatus>;
  onRefreshSessionStatus: (siteUrl?: string) => Promise<void>;
  onClearSession: (siteUrl: string) => Promise<void>;
  onOpenVerificationPage: (url: string) => void | Promise<void>;
  onRefreshAllAvatars: () => Promise<AvatarRefreshSummary>;
  onClose: () => void;
  onSave: () => void;
}) {
  const [sessionBusy, setSessionBusy] = useState(false);
  const [manualCookie, setManualCookie] = useState("");
  const [sessionMessage, setSessionMessage] = useState("");

  function update(field: keyof LibrarySettings, value: string) {
    setDraft({
      ...draft,
      [field]: value,
      ...(field === "rootPath"
        ? {
            downloadPath: joinWinPath(value, DOWNLOAD_FOLDER),
            appreciationPath: joinWinPath(value, APPRECIATION_FOLDER),
            onlineMetadataEnabled: draft.onlineMetadataEnabled,
            coverCapturePercent: draft.coverCapturePercent,
            metadataSiteUrl: draft.metadataSiteUrl,
            metadataBrowser: draft.metadataBrowser
          }
        : {})
    });
  }

  const verificationUrl = buildMetadataSearchUrl(draft.metadataSiteUrl || DEFAULT_METADATA_SITE, "IPZZ-832");

  async function handleImportSession() {
    setSessionBusy(true);
    setSessionMessage("");
    try {
      const result = await onImportSession(draft.metadataSiteUrl, draft.metadataBrowser);
      setSessionMessage(result.message);
    } catch (err) {
      setSessionMessage(formatError(err));
    } finally {
      setSessionBusy(false);
    }
  }

  async function handleSaveManualSession() {
    setSessionBusy(true);
    setSessionMessage("");
    try {
      const result = await onSaveManualSession(draft.metadataSiteUrl, manualCookie);
      setSessionMessage(result.message);
      setManualCookie("");
    } catch (err) {
      setSessionMessage(formatError(err));
    } finally {
      setSessionBusy(false);
    }
  }

  async function handleRefreshSessionStatus() {
    setSessionBusy(true);
    setSessionMessage("");
    try {
      await onRefreshSessionStatus(draft.metadataSiteUrl);
    } catch (err) {
      setSessionMessage(formatError(err));
    } finally {
      setSessionBusy(false);
    }
  }

  async function handleClearSession() {
    setSessionBusy(true);
    setSessionMessage("");
    try {
      await onClearSession(draft.metadataSiteUrl);
      setSessionMessage("已清除浏览器会话");
    } catch (err) {
      setSessionMessage(formatError(err));
    } finally {
      setSessionBusy(false);
    }
  }

  async function handleRefreshAllAvatars() {
    setSessionBusy(true);
    setSessionMessage("");
    try {
      const result = await onRefreshAllAvatars();
      if (result.total === 0) {
        setSessionMessage("当前没有可刷新的演员");
      } else if (result.failed === 0) {
        setSessionMessage(`头像刷新完成：成功 ${result.success} / ${result.total}`);
      } else {
        setSessionMessage(
          `头像刷新完成：成功 ${result.success}，失败 ${result.failed}${result.lastError ? `，最近错误：${result.lastError}` : ""}`
        );
      }
    } catch (err) {
      setSessionMessage(formatError(err));
    } finally {
      setSessionBusy(false);
    }
  }

  return (

    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="settings-modal" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
        <div className="modal-head">
          <div>
            <h2>设置</h2>
            <p>路径设置会影响扫描、预览、执行和播放。</p>
          </div>
          <button className="icon-button" onClick={onClose}>×</button>
        </div>
        <div className="settings-fields">
          <PathField
            label="根目录"
            value={draft.rootPath}
            onChange={(value) => update("rootPath", value)}
            onChoose={() => onChooseFolder("rootPath", draft.rootPath)}
          />
          <PathField
            label="下载区"
            value={draft.downloadPath}
            onChange={(value) => update("downloadPath", value)}
            onChoose={() => onChooseFolder("downloadPath", draft.downloadPath || draft.rootPath)}
          />
          <PathField
            label="欣赏区"
            value={draft.appreciationPath}
            onChange={(value) => update("appreciationPath", value)}
            onChoose={() => onChooseFolder("appreciationPath", draft.appreciationPath || draft.rootPath)}
          />
          <NumberField
            label="封面截取进度 (%)"
            value={draft.coverCapturePercent}
            min={0}
            max={95}
            onChange={(value) => setDraft({ ...draft, coverCapturePercent: clampCoverCapturePercent(value) })}
          />
          <OptionRow
            checked={draft.onlineMetadataEnabled}
            label="启用联网获取演员头像和作品信息"
            onChange={(checked) => setDraft({ ...draft, onlineMetadataEnabled: checked })}
          />
          <PathField
            label="元数据站点"
            value={draft.metadataSiteUrl}
            onChange={(value) => update("metadataSiteUrl", value)}
            onChoose={() => undefined}
            hideChoose
          />
          <SelectField
            label="会话来源"
            value={draft.metadataBrowser}
            onChange={(value) => setDraft({ ...draft, metadataBrowser: value as LibrarySettings["metadataBrowser"] })}
            options={[
              { label: "自动", value: "auto" },
              { label: "Edge", value: "edge" },
              { label: "Chrome", value: "chrome" }
            ]}
          />
          <div className="settings-card">
            <div className="settings-card-head">
              <strong>JavBus 会话</strong>
              <span className={metadataSession?.status === "verified" ? "pill good" : metadataSession?.hasCookie ? "pill warn" : "pill"}>
                {metadataSession?.status === "verified" ? "已验证" : metadataSession?.hasCookie ? "待验证" : "未导入"}
              </span>
            </div>
            <div className="settings-note">
              <div>来源：{metadataSession?.source || "未设置"}</div>
              <div>状态：{metadataSession?.message || "未导入浏览器会话"}</div>
              {metadataSession?.updatedAt ? <div>更新时间：{metadataSession.updatedAt}</div> : null}
            </div>
            <div className="settings-button-row">
              <button className="primary-button" type="button" onClick={handleImportSession} disabled={sessionBusy}>
                {sessionBusy ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
                <span>导入浏览器会话</span>
              </button>
              <button className="ghost-button" type="button" onClick={() => onOpenVerificationPage(verificationUrl)} disabled={sessionBusy}>
                <span>打开验证页</span>
              </button>
              <button className="ghost-button" type="button" onClick={handleRefreshSessionStatus} disabled={sessionBusy}>
                <span>刷新状态</span>
              </button>
              <button
                className="ghost-button"
                type="button"
                onClick={handleRefreshAllAvatars}
                disabled={sessionBusy || actorCount === 0 || !draft.onlineMetadataEnabled}
              >
                <span>{`刷新全部头像${actorCount > 0 ? ` (${actorCount})` : ""}`}</span>
              </button>
              <button className="ghost-button" type="button" onClick={handleClearSession} disabled={sessionBusy || !metadataSession?.hasCookie}>
                <span>清除会话</span>
              </button>
            </div>
            <label className="path-field">
              <span>手动 Cookie</span>
              <div className="stack-field">
                <textarea value={manualCookie} onChange={(event) => setManualCookie(event.target.value)} placeholder="粘贴浏览器请求头里的 Cookie 内容" />
                <button className="ghost-button" type="button" onClick={handleSaveManualSession} disabled={sessionBusy || !manualCookie.trim()}>
                  <span>保存手动会话</span>
                </button>
              </div>
            </label>
            {sessionMessage ? <div className="muted-box">{sessionMessage}</div> : null}
          </div>
        </div>
        <div className="modal-actions">
          <button className="ghost-button" onClick={onClose}>取消</button>
          <button className="primary-button" onClick={onSave}>保存设置</button>
        </div>
      </section>
    </div>
  );
}

function PathField({
  label,
  value,
  onChange,
  onChoose,
  hideChoose = false
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  onChoose: () => void;
  hideChoose?: boolean;
}) {
  return (
    <label className="path-field">
      <span>{label}</span>
      <div>
        <input value={value} onChange={(event) => onChange(event.target.value)} />
        {hideChoose ? null : (
          <button className="icon-button" type="button" onClick={onChoose} title="选择文件夹">
            <Search size={17} />
          </button>
        )}
      </div>
    </label>
  );
}

function NumberField({
  label,
  value,
  min,
  max,
  onChange
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="path-field">
      <span>{label}</span>
      <div className="number-field">
        <input
          type="range"
          min={min}
          max={max}
          step={1}
          value={value}
          onChange={(event) => onChange(Number(event.target.value))}
        />
        <input
          type="number"
          min={min}
          max={max}
          step={1}
          value={value}
          onChange={(event) => onChange(Number(event.target.value))}
        />
      </div>
    </label>
  );
}

function SelectField({
  label,
  value,
  onChange,
  options
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: Array<{ label: string; value: string }>;
}) {
  return (
    <label className="path-field">
      <span>{label}</span>
      <div>
        <select value={value} onChange={(event) => onChange(event.target.value)}>
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>
    </label>
  );
}

function SortSelect({
  value,
  direction,
  options,
  onChange,
  onToggleDirection
}: {
  value: string;
  direction: SortDirection;
  options: Array<{ label: string; value: string }>;
  onChange: (value: string) => void;
  onToggleDirection: () => void;
}) {
  return (
    <div className="sort-control">
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <button
        className="icon-button sort-direction-button"
        type="button"
        onClick={onToggleDirection}
        title={direction === "desc" ? "降序" : "升序"}
      >
        {direction === "desc" ? <ArrowDown size={16} /> : <ArrowUp size={16} />}
      </button>
    </div>
  );
}

let activeCoverRequests = 0;
const coverRequestQueue: Array<() => void> = [];

function runNextCoverRequest() {
  if (activeCoverRequests >= COVER_REQUEST_CONCURRENCY) return;
  const next = coverRequestQueue.shift();
  next?.();
}

function requestVideoCover(rootPath: string, filePath: string, coverCapturePercent: number) {
  let cancelled = false;

  const promise = new Promise<string | null>((resolve, reject) => {
    const run = () => {
      if (cancelled) {
        resolve(null);
        runNextCoverRequest();
        return;
      }

      activeCoverRequests += 1;
      invoke<string | null>("ensure_video_cover", {
        rootPath,
        filePath,
        coverPercent: coverCapturePercent
      })
        .then(resolve, reject)
        .finally(() => {
          activeCoverRequests = Math.max(0, activeCoverRequests - 1);
          runNextCoverRequest();
        });
    };

    if (activeCoverRequests < COVER_REQUEST_CONCURRENCY) {
      run();
    } else {
      coverRequestQueue.push(run);
    }
  });

  return {
    promise,
    cancel: () => {
      cancelled = true;
    }
  };
}

function VideoGrid({
  rootPath,
  coverCapturePercent,
  videos,
  storageKey,
  onOpenVideo
}: {
  rootPath: string;
  coverCapturePercent: number;
  videos: ActorVideo[];
  storageKey: string;
  onOpenVideo: (path: string) => void;
}) {
  const gridRef = useRef<HTMLDivElement | null>(null);
  const [scrollRoot, setScrollRoot] = useState<HTMLDivElement | null>(null);
  const [gridColumns, setGridColumns] = useState(3);
  const [coverHeight, setCoverHeight] = useState(96);
  const [sort, setSort] = useState<VideoSort>(() => loadStoredVideoView(storageKey).sort);
  const [sortDirection, setSortDirection] = useState<SortDirection>(() => loadStoredVideoView(storageKey).direction);
  const [captionFilter, setCaptionFilter] = useState(() => loadStoredVideoView(storageKey).captionFilter);
  const [uncensoredFilter, setUncensoredFilter] = useState(() => loadStoredVideoView(storageKey).uncensoredFilter);
  const skipNextVideoViewSaveRef = useRef(false);

  useEffect(() => {
    setScrollRoot(gridRef.current);
  }, [storageKey]);
  useEffect(() => {
    const stored = loadStoredVideoView(storageKey);
    setSort(stored.sort);
    setSortDirection(stored.direction);
    setCaptionFilter(stored.captionFilter);
    setUncensoredFilter(stored.uncensoredFilter);
  }, [storageKey]);

  useEffect(() => {
    saveStoredVideoView(storageKey, { sort, direction: sortDirection, captionFilter, uncensoredFilter });
  }, [storageKey, sort, sortDirection, captionFilter, uncensoredFilter]);

  useEffect(() => {
    gridRef.current?.scrollTo({ top: 0 });
  }, [storageKey, sort, sortDirection, captionFilter, uncensoredFilter]);

  useEffect(() => {
    const grid = gridRef.current;
    if (!grid) return;

    const update = () => {
      const width = grid.clientWidth;
      const height = grid.clientHeight;

      if (width <= 0 || height <= 0) {
        setGridColumns(3);
        setCoverHeight(96);
        return;
      }

      const availableHeightForThreeRows =
        height -
        VIDEO_GRID_GAP * (VIDEO_GRID_MIN_ROWS - 1) -
        VIDEO_META_HEIGHT * VIDEO_GRID_MIN_ROWS;

      const nextCoverHeight = Math.max(
        VIDEO_MIN_COVER_HEIGHT,
        Math.min(
          VIDEO_MAX_COVER_HEIGHT,
          Math.floor(availableHeightForThreeRows / VIDEO_GRID_MIN_ROWS)
        )
      );

      const minCardWidth = 132;
      const preferredCardWidth = 210;

      const maxColumnsByMinWidth = Math.max(
        VIDEO_GRID_MIN_COLUMNS,
        Math.floor((width + VIDEO_GRID_GAP) / (minCardWidth + VIDEO_GRID_GAP))
      );

      const preferredColumns = Math.max(
        VIDEO_GRID_MIN_COLUMNS,
        Math.floor((width + VIDEO_GRID_GAP) / (preferredCardWidth + VIDEO_GRID_GAP))
      );

      const nextColumns = Math.max(
        VIDEO_GRID_MIN_COLUMNS,
        Math.min(maxColumnsByMinWidth, preferredColumns || VIDEO_GRID_MIN_COLUMNS)
      );

      setGridColumns(nextColumns);
      setCoverHeight(nextCoverHeight);
    };

    update();

    const observer = new ResizeObserver(update);
    observer.observe(grid);
    window.addEventListener("resize", update);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", update);
    };
  }, []);

  const filteredVideos = useMemo(() => {
    if (!captionFilter && !uncensoredFilter) return videos;
    return videos.filter((video) => {
      const flags = videoSuffixFlags(video.name);
      if (captionFilter && !flags.captions) return false;
      if (uncensoredFilter && !flags.uncensored) return false;
      return true;
    });
  }, [videos, captionFilter, uncensoredFilter]);
  const sorted = useMemo(() => sortVideos(filteredVideos, sort, sortDirection), [filteredVideos, sort, sortDirection]);
  const filterActive = captionFilter || uncensoredFilter;

  if (videos.length === 0) return <EmptyLine text="没有视频" />;

  return (
    <div className="video-section">
      <div className="video-toolbar">
        <SortSelect
          value={sort}
          direction={sortDirection}
          onChange={(value) => setSort(value as VideoSort)}
          onToggleDirection={() => setSortDirection((current) => (current === "desc" ? "asc" : "desc"))}
          options={[
            { label: "按时间排序", value: "modified" },
            { label: "按大小排序", value: "size" },
            { label: "按名称排序", value: "name" },
            { label: "按番号排序", value: "code" }
          ]}
        />
        <div className="video-filter-group" aria-label="视频筛选">
          <button
            className={captionFilter ? "video-filter active" : "video-filter"}
            type="button"
            aria-pressed={captionFilter}
            onClick={() => setCaptionFilter((current) => !current)}
          >
            字幕
          </button>
          <button
            className={uncensoredFilter ? "video-filter active uncensored" : "video-filter"}
            type="button"
            aria-pressed={uncensoredFilter}
            onClick={() => setUncensoredFilter((current) => !current)}
          >
            无码
          </button>
        </div>
        <span>{filterActive ? `${sorted.length} / ${videos.length} 个视频` : `${sorted.length} 个视频`}</span>
      </div>

      <div
        className="video-grid"
        ref={gridRef}
        style={
          {
            "--video-columns": String(gridColumns),
            "--video-cover-height": `${coverHeight}px`,
            "--video-meta-height": `${VIDEO_META_HEIGHT}px`
          } as React.CSSProperties
        }
      >
        {sorted.length === 0 ? (
          <div className="video-grid-empty">
            <EmptyLine text="没有匹配的视频" />
          </div>
        ) : (
          sorted.map((video) => (
            <VideoCard
              rootPath={rootPath}
              coverCapturePercent={coverCapturePercent}
              scrollRoot={scrollRoot}
              video={video}
              onOpenVideo={onOpenVideo}
              key={video.path}
            />
          ))
        )}
      </div>
    </div>
  );
}

function VideoCard({
  rootPath,
  coverCapturePercent,
  scrollRoot,
  video,
  onOpenVideo
}: {
  rootPath: string;
  coverCapturePercent: number;
  scrollRoot: HTMLDivElement | null;
  video: ActorVideo;
  onOpenVideo: (path: string) => void;
}) {
  const code = videoDisplayCode(video);
  const flags = videoSuffixFlags(video.name);

  return (
    <article
      className="video-card"
      onClick={() => onOpenVideo(video.path)}
      aria-label={`播放 ${video.name}`}
      role="button"
      tabIndex={0}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onOpenVideo(video.path);
        }
      }}
    >
      <VideoCover
        rootPath={rootPath}
        coverCapturePercent={coverCapturePercent}
        scrollRoot={scrollRoot}
        video={video}
      />
      <div className="video-meta" title={video.name}>
        <div className="video-code-row">
          <strong className="video-code" title={code}>{code}</strong>
          <span className={flags.captions ? "video-flag active" : "video-flag"}>字幕</span>
          <span className={flags.uncensored ? "video-flag active uncensored" : "video-flag"}>无码</span>
        </div>
        <div className="video-detail-row">
          <span title={video.modified}>{video.modified}</span>
          <span>{formatNumber(video.sizeGb)} GB</span>
        </div>
      </div>
    </article>
  );
}

function VideoCover({
  rootPath,
  coverCapturePercent,
  scrollRoot,
  video,
  eager = false
}: {
  rootPath: string;
  coverCapturePercent: number;
  scrollRoot: HTMLDivElement | null;
  video: ActorVideo;
  eager?: boolean;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const requestIdRef = useRef(0);
  const [coverPath, setCoverPath] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const coverSrc = coverPath ? convertFileSrc(coverPath) : "";

  useEffect(() => {
    setCoverPath(null);
    setFailed(false);
    requestIdRef.current += 1;
  }, [video.path, video.coverPath, coverCapturePercent]);

  useEffect(() => {
    const host = hostRef.current;
    const root = scrollRoot;
    if (!host || (!root && !eager)) return;

    let cancelled = false;
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    let coverRequest: ReturnType<typeof requestVideoCover> | null = null;

    const loadCover = async () => {
      try {
        coverRequest = requestVideoCover(rootPath, video.path, coverCapturePercent);
        const result = await coverRequest.promise;
        if (cancelled || requestIdRef.current !== requestId) return;
        if (result) {
          setCoverPath(result);
          setFailed(false);
        } else {
          setFailed(true);
        }
      } catch {
        if (!cancelled && requestIdRef.current === requestId) {
          setFailed(true);
        }
      }
    };

    if (eager) {
      void loadCover();
      return () => {
        cancelled = true;
        coverRequest?.cancel();
      };
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer.disconnect();
        void loadCover();
      },
      {
        root,
        rootMargin: "320px 0px",
        threshold: 0.01
      }
    );

    observer.observe(host);

    return () => {
      cancelled = true;
      coverRequest?.cancel();
      observer.disconnect();
    };
  }, [rootPath, coverCapturePercent, scrollRoot, video.path, video.coverPath, eager]);

  return (
    <div className="cover" ref={hostRef} style={visualStyle(video.name)}>
      {coverSrc ? (
        <img
          src={coverSrc}
          alt=""
          loading="lazy"
          onError={() => {
            setCoverPath(null);
            setFailed(true);
          }}
        />
      ) : null}
      {!coverSrc ? (
        <div className={failed ? "cover-fallback failed" : "cover-fallback"}>
          <strong>{videoDisplayCode(video)}</strong>
          <span>暂无封面</span>
        </div>
      ) : null}
    </div>
  );
}

function OptionRow({ checked, label, onChange }: { checked: boolean; label: string; onChange: (checked: boolean) => void }) {
  return (
    <label className="option-row">
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      <span>{label}</span>
    </label>
  );
}

function MetaItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="meta-item">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function FileCell({ title, subtitle }: { title: string; subtitle?: string }) {
  return (
    <div className="file-main">
      <strong title={title}>{title}</strong>
      {subtitle ? <span title={subtitle}>{subtitle}</span> : null}
    </div>
  );
}

function StatusPill({ status }: { status: string }) {
  const label =
    status === "ready"
      ? "可执行"
      : status === "conflict"
        ? "冲突"
        : status === "success"
          ? "成功"
          : status === "failed"
            ? "失败"
            : "跳过";
  const tone = status === "ready" || status === "success" ? "good" : status === "conflict" || status === "failed" ? "danger" : "warn";
  return <span className={`pill ${tone}`}>{label}</span>;
}

function EmptyState({ onScan, isBusy, isBooting }: { onScan: () => void; isBusy: boolean; isBooting: boolean }) {
  return (
    <section className="empty-state">
      <Library size={40} />
      <h1>Kawa Library</h1>
      <p>{isBooting ? "正在读取数据库缓存。" : "先在设置里确认路径，然后扫描根目录。"}</p>
      <button className="primary-button" onClick={onScan} disabled={isBusy}>
        {isBusy ? <Loader2 className="spin" size={18} /> : <RefreshCw size={18} />}
        <span>{isBusy ? "加载中" : "扫描"}</span>
      </button>
    </section>
  );
}

function EmptyLine({ text }: { text: string }) {
  return <div className="empty-line">{text}</div>;
}

function normalizeReport(report: ScanReport): ScanReport {
  return {
    ...report,
    actors: (report.actors ?? []).map((actor) => ({
      ...actor,
      videos: (actor.videos ?? []).map((video) => ({
        ...video,
        modifiedUnix: video.modifiedUnix || Math.floor(modifiedValue(video.modified) / 1000),
        coverPath: video.coverPath ?? null
      }))
    })),
    appreciationVideos: (report.appreciationVideos ?? []).map((file) => ({
      ...file,
      coverPath: file.coverPath ?? null
    })),
    rootFiles: (report.rootFiles ?? []).map((file) => ({
      ...file,
      coverPath: file.coverPath ?? null
    })),
    nameDuplicates: report.nameDuplicates ?? [],
    codeDuplicates: report.codeDuplicates ?? [],
    numericDuplicates: report.numericDuplicates ?? [],
    downloadMovePlans: report.downloadMovePlans ?? []
  };
}

function buildDedupDisplayGroups(report: ScanReport): Record<DedupKind, DedupDisplayGroup[]> {
  const code = (report.codeDuplicates ?? [])
    .map((group) => duplicateGroupToDisplay(group, "code", "相同番号"))
    .sort(sortDedupDisplayGroups);
  const name = (report.nameDuplicates ?? [])
    .map((group) => duplicateGroupToDisplay(group, "name", "相同文件名"))
    .sort(sortDedupDisplayGroups);
  const numeric = (report.numericDuplicates ?? [])
    .map((group) => {
      const firstFile = group.files[0];
      const folderPath = firstFile ? parentPath(firstFile.path) : group.folder;
      return {
        id: `numeric:${group.folder}:${group.number}`,
        kind: "numeric" as const,
        key: group.number,
        title: group.number,
        subtitle: folderPath ? `${group.folder} · ${folderPath}` : group.folder,
        count: group.count || group.files.length,
        totalSizeGb: sumFileSizeGb(group.files),
        files: group.files,
        highlight: group.number,
        latestModifiedUnix: groupLatestModifiedUnix(group.files)
      };
    })
    .sort(sortDedupDisplayGroups);
  return { code, name, numeric };
}

function duplicateGroupToDisplay(group: DuplicateGroup, kind: Exclude<DedupKind, "numeric">, subtitle: string): DedupDisplayGroup {
  const files = [...group.files].sort((left, right) => libraryFileModifiedValue(right) - libraryFileModifiedValue(left) || left.name.localeCompare(right.name, "zh-Hans-CN"));
  return {
    id: `${kind}:${group.key}`,
    kind,
    key: group.key,
    title: group.key,
    subtitle,
    count: group.count || files.length,
    totalSizeGb: group.totalSizeGb > 0 ? group.totalSizeGb : sumFileSizeGb(files),
    files,
    highlight: group.key,
    latestModifiedUnix: groupLatestModifiedUnix(files)
  };
}

function sortDedupDisplayGroups(left: DedupDisplayGroup, right: DedupDisplayGroup) {
  return right.latestModifiedUnix - left.latestModifiedUnix || right.count - left.count || right.totalSizeGb - left.totalSizeGb || left.title.localeCompare(right.title, "zh-Hans-CN");
}

function uniqueFilesFromDedupGroups(groups: DedupDisplayGroup[]) {
  const byPath = new Map<string, LibraryFile>();
  for (const group of groups) {
    for (const file of group.files) {
      if (!byPath.has(file.path)) byPath.set(file.path, file);
    }
  }
  return [...byPath.values()];
}

function firstAvailableDedupKind(groups: Record<DedupKind, DedupDisplayGroup[]>): DedupKind {
  if (groups.code.length > 0) return "code";
  if (groups.name.length > 0) return "name";
  if (groups.numeric.length > 0) return "numeric";
  return "code";
}

function sumFileSizeGb(files: LibraryFile[]) {
  return files.reduce((total, file) => total + (Number.isFinite(file.sizeGb) ? file.sizeGb : 0), 0);
}

function groupLatestModifiedUnix(files: LibraryFile[]) {
  return files.reduce((latest, file) => Math.max(latest, file.modifiedUnix || Math.floor(modifiedValue(file.modified) / 1000)), 0);
}

function isIgnoredDedupGroup(group: DedupDisplayGroup, ignoredAtByKey: Record<string, number>) {
  const ignoredAt = ignoredAtByKey[group.key];
  return typeof ignoredAt === "number" && group.latestModifiedUnix * 1000 <= ignoredAt;
}

function isMultipartSuffixDedupGroup(group: DedupDisplayGroup) {
  if (group.kind !== "code" || group.files.length < 2) return false;
  const suffixes = group.files.map((file) => multipartSuffixNumber(file.name, group.key));
  if (suffixes.some((suffix) => suffix === null)) return false;
  const uniqueSuffixes = new Set(suffixes);
  return uniqueSuffixes.size === suffixes.length && uniqueSuffixes.size >= 2;
}

function multipartSuffixNumber(fileName: string, code: string) {
  const codeParts = code.toUpperCase().match(/^([A-Z0-9]+)-(\d{2,8})$/);
  if (!codeParts) return null;
  const [, prefix, number] = codeParts;
  const stem = fileStem(fileName)
    .toUpperCase()
    .replace(/\[[^\]]*]/g, " ")
    .replace(/\([^)]*\)/g, " ");
  const tokens = stem.split(/[^A-Z0-9]+/).filter(Boolean);

  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index] !== number) continue;
    const prefixBeforeNumber = tokens.slice(0, index).join("");
    if (!prefixBeforeNumber.endsWith(prefix)) continue;
    const suffix = tokens[index + 1];
    if (!suffix || !/^\d{1,2}$/.test(suffix)) return null;
    const value = Number(suffix);
    return value >= 1 && value <= 20 ? value : null;
  }
  return null;
}

function groupNumericDuplicatesByFolder(groups: NumericDuplicateGroup[]) {
  const byFolder = new Map<string, { folder: string; path: string; groups: NumericDuplicateGroup[] }>();
  for (const group of groups) {
    const firstFile = group.files[0];
    const folderPath = firstFile ? parentPath(firstFile.path) : "";
    const existing = byFolder.get(group.folder);
    if (existing) {
      existing.groups.push(group);
    } else {
      byFolder.set(group.folder, { folder: group.folder, path: folderPath, groups: [group] });
    }
  }
  return [...byFolder.values()].sort((left, right) => right.groups.length - left.groups.length || left.folder.localeCompare(right.folder, "zh-Hans-CN"));
}

function highlightText(text: string, keyword: string) {
  if (!keyword) return text;
  const index = text.toLocaleLowerCase().indexOf(keyword.toLocaleLowerCase());
  if (index < 0) return text;
  return (
    <>
      {text.slice(0, index)}
      <b>{text.slice(index, index + keyword.length)}</b>
      {text.slice(index + keyword.length)}
    </>
  );
}

function highlightNumber(name: string, number: string) {
  const index = name.indexOf(number);
  if (index < 0) return name;
  return (
    <>
      {name.slice(0, index)}
      <b>{number}</b>
      {name.slice(index + number.length)}
    </>
  );
}

function parentPath(path: string) {
  const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return index > 0 ? path.slice(0, index) : path;
}

function displayPath(path: string | null | undefined) {
  if (!path) return "";
  return path
    .replace(/^\\\\\?\\UNC\\/i, "\\\\")
    .replace(/^\\\\\?\\/i, "");
}

function libraryFileToActorVideo(file: LibraryFile): ActorVideo {
  return {
    path: file.path,
    name: file.name,
    sizeGb: file.sizeGb,
    modified: file.modified,
    modifiedUnix: file.modifiedUnix,
    mediaCode: file.mediaCode,
    coverPath: file.coverPath ?? null
  };
}

function normalizeArchiveCode(fileName: string) {
  const stem = fileStem(fileName)
    .replace(/@.*$/g, "")
    .replace(/\[[^\]]*]/g, " ")
    .replace(/\([^)]*\)/g, " ")
    .replace(/[_\s]+/g, "-")
    .toUpperCase();
  const fc2 = stem.match(/FC2[-_ ]?(?:PPV)?[-_ ]?(\d{5,8})/i);
  if (fc2) return `FC2PPV-${fc2[1]}`;
  const code = stem.match(/([A-Z]{2,8})[-_ ]?(\d{2,6})/);
  if (!code) return "";
  return `${code[1]}-${code[2]}`;
}

function videoDisplayCode(video: ActorVideo) {
  return video.mediaCode || normalizeArchiveCode(video.name) || video.name;
}

function videoSuffixFlags(fileName: string) {
  const stem = fileStem(fileName).toUpperCase();
  const hasUc = /(^|[^A-Z0-9])UC($|[^A-Z0-9])/.test(stem);
  return {
    captions: hasUc || /(^|[^A-Z0-9])C($|[^A-Z0-9])/.test(stem),
    uncensored: hasUc || /(^|[^A-Z0-9])U($|[^A-Z0-9])/.test(stem)
  };
}

function matchActorsByCode(actors: ActorSummary[], code: string) {
  const compact = compactCode(code);
  return actors
    .filter((actor) => actor.videos.some((video) => compactCode(video.mediaCode ?? normalizeArchiveCode(video.name)) === compact))
    .slice(0, 8);
}

function uniqueArchiveCodes(videos: ActorVideo[]) {
  return [...new Set(videos.map((video) => normalizeArchiveCode(video.name)).filter(Boolean))];
}

function compactCode(value: string) {
  return value.replace(/[^A-Z0-9]/gi, "").toUpperCase();
}

function isFc2ArchiveCode(code: string) {
  return compactCode(code).startsWith("FC2PPV");
}

function buildMetadataSearchUrl(siteUrl: string, code: string) {
  const base = siteUrl.trim() || DEFAULT_METADATA_SITE;
  const normalizedBase = /^https?:\/\//i.test(base) ? base : `https://${base}`;
  return `${normalizedBase.replace(/\/+$/, "")}/search/${encodeURIComponent(code)}`;
}

function clampCoverCapturePercent(value: unknown) {
  const numeric = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(numeric)) return DEFAULT_COVER_CAPTURE_PERCENT;
  return Math.max(0, Math.min(95, Math.round(numeric)));
}

function isProcessingOptions(value: unknown): value is Partial<ProcessingOptions> {
  return typeof value === "object" && value !== null;
}

function loadStoredProcessingOptions(): ProcessingOptions {
  try {
    const raw = window.localStorage.getItem(PROCESSING_OPTIONS_STORAGE_KEY);
    if (!raw) return defaultOptions;
    const parsed = JSON.parse(raw);
    if (!isProcessingOptions(parsed)) return defaultOptions;
    return {
      ...defaultOptions,
      ...Object.fromEntries(
        Object.keys(defaultOptions).map((key) => [key, Boolean((parsed as Record<string, unknown>)[key])])
      )
    } as ProcessingOptions;
  } catch {
    return defaultOptions;
  }
}

function saveStoredProcessingOptions(options: ProcessingOptions) {
  try {
    window.localStorage.setItem(PROCESSING_OPTIONS_STORAGE_KEY, JSON.stringify(options));
  } catch {
    // 处理选项只是本地偏好。
  }
}

type ActorViewState = { sort: ActorSort; direction: SortDirection };
type VideoViewState = { sort: VideoSort; direction: SortDirection; captionFilter: boolean; uncensoredFilter: boolean };
type DedupViewState = { showMultipartGroups: boolean; showIgnoredGroups: boolean };

const defaultActorView: ActorViewState = { sort: "modified", direction: "desc" };
const defaultVideoView: VideoViewState = { sort: "modified", direction: "desc", captionFilter: false, uncensoredFilter: false };
const defaultDedupView: DedupViewState = { showMultipartGroups: false, showIgnoredGroups: false };

function normalizeSortDirection(value: unknown, fallback: SortDirection): SortDirection {
  return value === "asc" || value === "desc" ? value : fallback;
}

function normalizeActorSort(value: unknown, fallback: ActorSort): ActorSort {
  return value === "modified" || value === "size" || value === "count" || value === "name" ? value : fallback;
}

function normalizeVideoSort(value: unknown, fallback: VideoSort): VideoSort {
  return value === "modified" || value === "size" || value === "name" || value === "code" ? value : fallback;
}

function loadStoredActorView(): ActorViewState {
  try {
    const raw = window.localStorage.getItem(ACTOR_VIEW_STORAGE_KEY);
    if (!raw) return defaultActorView;
    const parsed = JSON.parse(raw) as Partial<ActorViewState>;
    return {
      sort: normalizeActorSort(parsed.sort, defaultActorView.sort),
      direction: normalizeSortDirection(parsed.direction, defaultActorView.direction)
    };
  } catch {
    return defaultActorView;
  }
}

function saveStoredActorView(value: ActorViewState) {
  try {
    window.localStorage.setItem(ACTOR_VIEW_STORAGE_KEY, JSON.stringify(value));
  } catch {
    // 演员排序只是本地偏好。
  }
}

function videoViewStorageKey(storageKey: string) {
  return `${VIDEO_VIEW_STORAGE_KEY_PREFIX}:${storageKey}`;
}

function loadStoredVideoView(storageKey: string): VideoViewState {
  try {
    const raw = window.localStorage.getItem(videoViewStorageKey(storageKey));
    if (!raw) return defaultVideoView;
    const parsed = JSON.parse(raw) as Partial<VideoViewState>;
    return {
      sort: normalizeVideoSort(parsed.sort, defaultVideoView.sort),
      direction: normalizeSortDirection(parsed.direction, defaultVideoView.direction),
      captionFilter: Boolean(parsed.captionFilter),
      uncensoredFilter: Boolean(parsed.uncensoredFilter)
    };
  } catch {
    return defaultVideoView;
  }
}

function saveStoredVideoView(storageKey: string, value: VideoViewState) {
  try {
    window.localStorage.setItem(videoViewStorageKey(storageKey), JSON.stringify(value));
  } catch {
    // 视频排序和筛选只是本地偏好。
  }
}

function loadStoredDedupView(): DedupViewState {
  try {
    const raw = window.localStorage.getItem(DEDUP_VIEW_STORAGE_KEY);
    if (!raw) return defaultDedupView;
    const parsed = JSON.parse(raw) as Partial<DedupViewState>;
    return {
      showMultipartGroups: Boolean(parsed.showMultipartGroups),
      showIgnoredGroups: Boolean(parsed.showIgnoredGroups)
    };
  } catch {
    return defaultDedupView;
  }
}

function saveStoredDedupView(value: DedupViewState) {
  try {
    window.localStorage.setItem(DEDUP_VIEW_STORAGE_KEY, JSON.stringify(value));
  } catch {
    // 查重显示选项只是本地偏好。
  }
}
function loadInitialSettings(): LibrarySettings {
  const fallback = {
    rootPath: DEFAULT_ROOT,
    downloadPath: joinWinPath(DEFAULT_ROOT, DOWNLOAD_FOLDER),
    appreciationPath: joinWinPath(DEFAULT_ROOT, APPRECIATION_FOLDER),
    coverCapturePercent: DEFAULT_COVER_CAPTURE_PERCENT,
    onlineMetadataEnabled: false,
    metadataSiteUrl: DEFAULT_METADATA_SITE,
    metadataBrowser: "auto" as const
  };

  try {
    const raw = window.localStorage.getItem(SETTINGS_STORAGE_KEY);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Partial<LibrarySettings>;
    const rootPath = parsed.rootPath?.trim() || fallback.rootPath;
    return {
      rootPath,
      downloadPath: parsed.downloadPath?.trim() || joinWinPath(rootPath, DOWNLOAD_FOLDER),
      appreciationPath: parsed.appreciationPath?.trim() || joinWinPath(rootPath, APPRECIATION_FOLDER),
      coverCapturePercent: clampCoverCapturePercent(parsed.coverCapturePercent ?? fallback.coverCapturePercent),
      onlineMetadataEnabled: parsed.onlineMetadataEnabled ?? fallback.onlineMetadataEnabled,
      metadataSiteUrl: parsed.metadataSiteUrl?.trim() || fallback.metadataSiteUrl,
      metadataBrowser: parsed.metadataBrowser ?? fallback.metadataBrowser
    };
  } catch {
    return fallback;
  }
}

function saveStoredSettings(settings: LibrarySettings) {
  try {
    window.localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // 本地设置写入失败不影响本次运行。
  }
}

function loadDedupIgnoreMap(storageKey: string) {
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, number>;
    return typeof parsed === "object" && parsed ? parsed : {};
  } catch {
    return {};
  }
}

function saveDedupIgnoreMap(storageKey: string, value: Record<string, number>) {
  try {
    window.localStorage.setItem(storageKey, JSON.stringify(value));
  } catch {
    // 忽略记录只是本地辅助状态。
  }
}

function downloadItemTitle(plan: DownloadMovePlan) {
  const title = lastPathSegment(plan.sourceName || plan.folder || plan.folderPath);
  return title.replace(/[\uFFFD\u0000-\u001F]/g, "");
}

function downloadItemSubtitle(plan: DownloadMovePlan) {
  const source = displayPath(plan.sourceName ?? plan.sourcePath ?? "");
  const parts = source.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 1) return undefined;
  return `来自 ${parts.slice(0, -1).join(" / ")}`;
}

function lastPathSegment(value: string) {
  const parts = displayPath(value).split(/[\\/]/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : value;
}

function sortActors(actors: ActorSummary[], sort: ActorSort, direction: SortDirection) {
  const sorted = [...actors];
  sorted.sort((left, right) => {
    let result = 0;
    if (sort === "size") result = left.totalSizeGb - right.totalSizeGb;
    else if (sort === "count") result = left.videos.length - right.videos.length;
    else if (sort === "name") result = left.name.localeCompare(right.name, "zh-Hans-CN");
    else result = left.modifiedUnix - right.modifiedUnix;
    if (result === 0) result = left.name.localeCompare(right.name, "zh-Hans-CN");
    return direction === "desc" ? -result : result;
  });
  return sorted;
}

function sortVideos(videos: ActorVideo[], sort: VideoSort, direction: SortDirection) {
  const sorted = [...videos];
  sorted.sort((left, right) => {
    let result = 0;
    if (sort === "size") result = left.sizeGb - right.sizeGb;
    else if (sort === "name") result = left.name.localeCompare(right.name, "zh-Hans-CN");
    else if (sort === "code") result = (left.mediaCode ?? left.name).localeCompare(right.mediaCode ?? right.name, "zh-Hans-CN");
    else result = videoModifiedValue(left) - videoModifiedValue(right);
    if (result === 0) result = left.name.localeCompare(right.name, "zh-Hans-CN");
    return direction === "desc" ? -result : result;
  });
  return sorted;
}

function videoModifiedValue(video: Pick<ActorVideo, "modified" | "modifiedUnix">) {
  return video.modifiedUnix ? video.modifiedUnix * 1000 : modifiedValue(video.modified);
}

function libraryFileModifiedValue(file: LibraryFile) {
  return file.modifiedUnix ? file.modifiedUnix * 1000 : modifiedValue(file.modified);
}

function modifiedValue(value: string) {
  const parsed = Date.parse(value.replace(" ", "T"));
  return Number.isNaN(parsed) ? 0 : parsed;
}

function visualStyle(seed: string) {
  const hue = hashString(seed) % 360;
  return {
    "--hue": String(hue),
    "--hue2": String((hue + 38) % 360)
  } as React.CSSProperties;
}

function hashString(value: string) {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }
  return hash;
}

function fileStem(name: string) {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

function joinWinPath(root: string, child: string) {
  return `${root.replace(/[\\/]+$/, "")}\\${child}`;
}

function formatNumber(value: number) {
  return new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 }).format(value);
}

function formatError(err: unknown) {
  if (err instanceof Error) return err.message;
  return String(err);
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
