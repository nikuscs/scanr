export const reply = (body: object) => console.log(JSON.stringify({ ver: 1, status: "ok", ...body }));

export const fail = (message: string): never => {
  console.log(JSON.stringify({ ver: 1, status: "error", message }));
  process.exit(1);
};
