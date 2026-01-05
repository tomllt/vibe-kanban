import { useQuery } from '@tanstack/react-query';
import { analyticsApi } from '@/lib/api';
import type {
  AnalyticsBucket,
  BurndownResponse,
  CfdResponse,
  CycleTimeResponse,
  DevExResponse,
} from 'shared/types';

type RangeParams = {
  projectId: string;
  days: number;
  bucket?: AnalyticsBucket;
};

export function useProjectBurndown(params: RangeParams & { includeCancelled?: boolean }) {
  return useQuery<BurndownResponse>({
    queryKey: [
      'analytics',
      'burndown',
      params.projectId,
      params.days,
      params.bucket ?? 'day',
      params.includeCancelled ?? false,
    ],
    queryFn: () =>
      analyticsApi.getBurndown({
        projectId: params.projectId,
        days: params.days,
        bucket: params.bucket,
        includeCancelled: params.includeCancelled,
      }),
    enabled: Boolean(params.projectId),
    staleTime: 30_000,
  });
}

export function useProjectCfd(params: RangeParams) {
  return useQuery<CfdResponse>({
    queryKey: ['analytics', 'cfd', params.projectId, params.days, params.bucket ?? 'day'],
    queryFn: () =>
      analyticsApi.getCfd({
        projectId: params.projectId,
        days: params.days,
        bucket: params.bucket,
      }),
    enabled: Boolean(params.projectId),
    staleTime: 30_000,
  });
}

export function useProjectCycleTime(params: RangeParams) {
  return useQuery<CycleTimeResponse>({
    queryKey: [
      'analytics',
      'cycle-time',
      params.projectId,
      params.days,
      params.bucket ?? 'day',
    ],
    queryFn: () =>
      analyticsApi.getCycleTime({
        projectId: params.projectId,
        days: params.days,
        bucket: params.bucket,
      }),
    enabled: Boolean(params.projectId),
    staleTime: 30_000,
  });
}

export function useProjectDevEx(params: RangeParams) {
  return useQuery<DevExResponse>({
    queryKey: ['analytics', 'devex', params.projectId, params.days, params.bucket ?? 'day'],
    queryFn: () =>
      analyticsApi.getDevEx({
        projectId: params.projectId,
        days: params.days,
        bucket: params.bucket,
      }),
    enabled: Boolean(params.projectId),
    staleTime: 30_000,
  });
}
