export default {
  fetch(request: Request) {
    const target = new URL(request.url);
    target.protocol = "https:";
    target.hostname = "loofah.io";
    target.port = "";
    return Response.redirect(target.toString(), 301);
  },
};
