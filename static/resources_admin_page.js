document.querySelectorAll(".button-view-initials").forEach((button) => {
  button.addEventListener("click", () => {
    const resourceId = button.getAttribute("resource-id");
    fetch(`/admin/resources/${resourceId}/initials`)
      .then((response) => {
        return response.text();
      })
      .then((html) => {
        document.getElementById("modalViewInitials-content").innerHTML = html;
        document.getElementById("modalViewInitials").showModal();
      })
      .catch((error) => {
        console.error(error);
        window.alert(`Something went wrong: ${error}`);
      });
  });
});

document.querySelectorAll(".button-edit-resource").forEach((button) => {
    button.addEventListener("click", () => {
      const row = button.closest("tr");
      document.getElementById("edit-resource-id").value = button.getAttribute("resource-id");
      const type = row.querySelector(".r-type").innerText;
      document.getElementById("edit-resource-file").style.setProperty("display", type !== "File" ? "none" : "block");
      document.getElementById("edit-resource-link").style.setProperty("display", type === "File" ? "none" : "block");
      document.getElementById("edit-resource-category").value = row.querySelector(".r-category").innerText;
      document.getElementById("edit-resource-name").value = row.querySelector(".r-name").innerText;
      if (type === "Link") {
        // pre-populate the link field
        document.getElementById("modalEditResource").querySelector("#edit-resource-link input").value = row.querySelector("a").innerText;
      }
      document.getElementById("modalEditResource").showModal();
    });
});

document.querySelectorAll(".button-delete-resource").forEach((button) => {
  button.addEventListener("click", () => {
    const resourceId = button.getAttribute("resource-id");
    const result = window.confirm("Are you sure you want to delete this resource?");
    if (result) {
      fetch(`/admin/resources/${resourceId}`, { method: "DELETE" })
        .then(() => {
          window.location.reload();
        })
        .catch((error) => {
          console.error(error);
          window.alert(`Something went wrong: ${error}`);
        });
    }
  });
});

document.getElementById("btn-modal-edit-close").addEventListener("click", (e) => {
  e.preventDefault();
  document.getElementById("modalEditResource").close();
});

document.getElementById("btn-modal-view-initials-close").addEventListener("click", (e) => {
  e.preventDefault();
  document.getElementById("modalViewInitials").close();
});
