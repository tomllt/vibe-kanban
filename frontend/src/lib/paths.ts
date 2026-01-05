export const paths = {
  projects: () => '/projects',
  projectTasks: (projectId: string) => `/projects/${projectId}/tasks`,
  projectAnalytics: (projectId: string) => `/projects/${projectId}/analytics`,
  task: (projectId: string, taskId: string) =>
    `/projects/${projectId}/tasks/${taskId}`,
  attempt: (projectId: string, taskId: string, attemptId: string) =>
    `/projects/${projectId}/tasks/${taskId}/attempts/${attemptId}`,
  attemptFull: (projectId: string, taskId: string, attemptId: string) =>
    `/projects/${projectId}/tasks/${taskId}/attempts/${attemptId}/full`,
};
