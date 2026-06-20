import type { GpuInfo, ServerConfig, ServerGpuData } from "../types/config";

export function gpuServerSortKey(
  server: Pick<ServerGpuData, "is_online" | "gpu_list">
): number {
  const hasCached =
    Array.isArray(server.gpu_list) && server.gpu_list.length > 0;
  if (server.is_online) return 2;
  if (hasCached) return 1;
  return 0;
}

export function orderGpuServersByConfig(
  servers: ServerGpuData[],
  configServers: ServerConfig[] | undefined
): ServerGpuData[] {
  const configOrder = (configServers || []).map((s) => s.host);
  return [...servers].sort((a, b) => {
    const idxA = configOrder.indexOf(a.host);
    const idxB = configOrder.indexOf(b.host);
    const orderA = idxA === -1 ? 9999 : idxA;
    const orderB = idxB === -1 ? 9999 : idxB;
    if (orderA !== orderB) return orderA - orderB;
    return gpuServerSortKey(b) - gpuServerSortKey(a);
  });
}

export function sortGpuJobGroups(
  groups: Record<string, GpuInfo[]>
): [string, GpuInfo[]][] {
  return Object.entries(groups)
    .sort(([jobA], [jobB]) => {
      if (jobA === "SYSTEM" && jobB !== "SYSTEM") return 1;
      if (jobB === "SYSTEM" && jobA !== "SYSTEM") return -1;
      return jobA.localeCompare(jobB);
    })
    .map(
      ([jobId, gpus]) =>
        [
          jobId,
          [...gpus].sort((a, b) => {
            const nameA = a.name || "";
            const nameB = b.name || "";
            return nameA.localeCompare(nameB, undefined, { numeric: true });
          }),
        ] as [string, GpuInfo[]]
    );
}
