import { TaskStatus } from 'shared/types';

export const statusLabels: Record<TaskStatus, string> = {
  todo: 'Local',
  inprogress: 'Feature',
  inreview: 'Staging',
  done: 'Prod',
  cancelled: 'Cancelled',
};

export const statusBoardColors: Record<TaskStatus, string> = {
  todo: '--neutral-foreground',
  inprogress: '--info',
  inreview: '--warning',
  done: '--success',
  cancelled: '--destructive',
};
